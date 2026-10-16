//! Leitor/gravador NBT mínimo e sem perdas (Java Edition, big-endian).
//! Usado no `servers.dat` (NBT sem compressão) e para conferir `level.dat` (NBT com gzip).
//! Preserva qualquer tag que não conhecemos, para nunca estragar o arquivo do jogador.

use std::io::{Read, Write};

#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    /// (tipo dos elementos, elementos)
    List(u8, Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    pub fn id(&self) -> u8 {
        match self {
            Tag::Byte(_) => 1,
            Tag::Short(_) => 2,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Float(_) => 5,
            Tag::Double(_) => 6,
            Tag::ByteArray(_) => 7,
            Tag::String(_) => 8,
            Tag::List(..) => 9,
            Tag::Compound(_) => 10,
            Tag::IntArray(_) => 11,
            Tag::LongArray(_) => 12,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Tag> {
        match self {
            Tag::Compound(v) => v.iter().find(|(k, _)| k == key).map(|(_, t)| t),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Tag> {
        match self {
            Tag::Compound(v) => v.iter_mut().find(|(k, _)| k == key).map(|(_, t)| t),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Tag::String(s) => Some(s),
            _ => None,
        }
    }

    /// Define (ou substitui) um campo de compound mantendo a posição.
    pub fn set(&mut self, key: &str, value: Tag) {
        if let Tag::Compound(v) = self {
            if let Some(slot) = v.iter_mut().find(|(k, _)| k == key) {
                slot.1 = value;
            } else {
                v.push((key.to_string(), value));
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NbtError {
    #[error("nbt: {0}")]
    Io(#[from] std::io::Error),
    #[error("nbt: tipo de tag inválido {0}")]
    BadTag(u8),
    #[error("nbt: texto inválido")]
    BadString,
    #[error("nbt: estrutura profunda demais")]
    TooDeep,
}

/// Lê o arquivo inteiro: (nome da raiz, compound raiz).
pub fn read(mut r: impl Read) -> Result<(String, Tag), NbtError> {
    let id = read_u8(&mut r)?;
    if id != 10 {
        return Err(NbtError::BadTag(id));
    }
    let name = read_string(&mut r)?;
    let tag = read_payload(&mut r, 10, 0)?;
    Ok((name, tag))
}

pub fn write(mut w: impl Write, name: &str, root: &Tag) -> Result<(), NbtError> {
    w.write_all(&[root.id()])?;
    write_string(&mut w, name)?;
    write_payload(&mut w, root)
}

pub fn read_gzip(r: impl Read) -> Result<(String, Tag), NbtError> {
    read(flate2::read::GzDecoder::new(r))
}

fn read_u8(r: &mut impl Read) -> std::io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

macro_rules! rd {
    ($r:expr, $t:ty, $n:expr) => {{
        let mut b = [0u8; $n];
        $r.read_exact(&mut b)?;
        <$t>::from_be_bytes(b)
    }};
}

fn read_len(r: &mut impl Read) -> Result<usize, NbtError> {
    let n = rd!(r, i32, 4);
    if !(0..=64 * 1024 * 1024).contains(&n) {
        return Err(NbtError::BadString);
    }
    Ok(n as usize)
}

fn read_string(r: &mut impl Read) -> Result<String, NbtError> {
    let n = rd!(r, u16, 2) as usize;
    let mut b = vec![0u8; n];
    r.read_exact(&mut b)?;
    decode_mutf8(&b).ok_or(NbtError::BadString)
}

fn write_string(w: &mut impl Write, s: &str) -> Result<(), NbtError> {
    let b = encode_mutf8(s);
    if b.len() > u16::MAX as usize {
        return Err(NbtError::BadString);
    }
    w.write_all(&(b.len() as u16).to_be_bytes())?;
    w.write_all(&b)?;
    Ok(())
}

fn read_payload(r: &mut impl Read, id: u8, depth: usize) -> Result<Tag, NbtError> {
    if depth > 512 {
        return Err(NbtError::TooDeep);
    }
    Ok(match id {
        1 => Tag::Byte(rd!(r, i8, 1)),
        2 => Tag::Short(rd!(r, i16, 2)),
        3 => Tag::Int(rd!(r, i32, 4)),
        4 => Tag::Long(rd!(r, i64, 8)),
        5 => Tag::Float(rd!(r, f32, 4)),
        6 => Tag::Double(rd!(r, f64, 8)),
        7 => {
            let n = read_len(r)?;
            let mut b = vec![0u8; n];
            r.read_exact(&mut b)?;
            Tag::ByteArray(b.into_iter().map(|x| x as i8).collect())
        }
        8 => Tag::String(read_string(r)?),
        9 => {
            let elem = read_u8(r)?;
            let n = read_len(r)?;
            if n > 0 && !(1..=12).contains(&elem) {
                return Err(NbtError::BadTag(elem));
            }
            let mut v = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                v.push(read_payload(r, elem, depth + 1)?);
            }
            Tag::List(elem, v)
        }
        10 => {
            let mut v = vec![];
            loop {
                let t = read_u8(r)?;
                if t == 0 {
                    break;
                }
                let name = read_string(r)?;
                v.push((name, read_payload(r, t, depth + 1)?));
            }
            Tag::Compound(v)
        }
        11 => {
            let n = read_len(r)?;
            let mut v = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                v.push(rd!(r, i32, 4));
            }
            Tag::IntArray(v)
        }
        12 => {
            let n = read_len(r)?;
            let mut v = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                v.push(rd!(r, i64, 8));
            }
            Tag::LongArray(v)
        }
        other => return Err(NbtError::BadTag(other)),
    })
}

fn write_payload(w: &mut impl Write, t: &Tag) -> Result<(), NbtError> {
    match t {
        Tag::Byte(v) => w.write_all(&v.to_be_bytes())?,
        Tag::Short(v) => w.write_all(&v.to_be_bytes())?,
        Tag::Int(v) => w.write_all(&v.to_be_bytes())?,
        Tag::Long(v) => w.write_all(&v.to_be_bytes())?,
        Tag::Float(v) => w.write_all(&v.to_be_bytes())?,
        Tag::Double(v) => w.write_all(&v.to_be_bytes())?,
        Tag::ByteArray(v) => {
            w.write_all(&(v.len() as i32).to_be_bytes())?;
            w.write_all(&v.iter().map(|x| *x as u8).collect::<Vec<_>>())?;
        }
        Tag::String(s) => write_string(w, s)?,
        Tag::List(elem, v) => {
            let elem = if v.is_empty() { *elem } else { v[0].id() };
            w.write_all(&[elem])?;
            w.write_all(&(v.len() as i32).to_be_bytes())?;
            for x in v {
                write_payload(w, x)?;
            }
        }
        Tag::Compound(v) => {
            for (k, x) in v {
                w.write_all(&[x.id()])?;
                write_string(w, k)?;
                write_payload(w, x)?;
            }
            w.write_all(&[0])?;
        }
        Tag::IntArray(v) => {
            w.write_all(&(v.len() as i32).to_be_bytes())?;
            for x in v {
                w.write_all(&x.to_be_bytes())?;
            }
        }
        Tag::LongArray(v) => {
            w.write_all(&(v.len() as i32).to_be_bytes())?;
            for x in v {
                w.write_all(&x.to_be_bytes())?;
            }
        }
    }
    Ok(())
}

/// "Modified UTF-8" do Java: NUL vira 0xC0 0x80 e caracteres fora do BMP viram pares substitutos.
fn encode_mutf8(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for u in s.encode_utf16() {
        match u {
            0x0001..=0x007F => out.push(u as u8),
            0x0000 | 0x0080..=0x07FF => {
                out.push(0xC0 | ((u >> 6) & 0x1F) as u8);
                out.push(0x80 | (u & 0x3F) as u8);
            }
            _ => {
                out.push(0xE0 | ((u >> 12) & 0x0F) as u8);
                out.push(0x80 | ((u >> 6) & 0x3F) as u8);
                out.push(0x80 | (u & 0x3F) as u8);
            }
        }
    }
    out
}

fn decode_mutf8(b: &[u8]) -> Option<String> {
    let mut units = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c & 0x80 == 0 {
            units.push(c as u16);
            i += 1;
        } else if c & 0xE0 == 0xC0 {
            let c2 = *b.get(i + 1)?;
            units.push((((c & 0x1F) as u16) << 6) | (c2 & 0x3F) as u16);
            i += 2;
        } else if c & 0xF0 == 0xE0 {
            let c2 = *b.get(i + 1)?;
            let c3 = *b.get(i + 2)?;
            units.push((((c & 0x0F) as u16) << 12) | (((c2 & 0x3F) as u16) << 6) | (c3 & 0x3F) as u16);
            i += 3;
        } else {
            return None;
        }
    }
    String::from_utf16(&units).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_everything() {
        let root = Tag::Compound(vec![
            ("b".into(), Tag::Byte(-1)),
            ("s".into(), Tag::String("Olá 🌍 \0".into())),
            ("l".into(), Tag::List(10, vec![Tag::Compound(vec![("x".into(), Tag::Int(7))])])),
            ("e".into(), Tag::List(0, vec![])),
            ("ia".into(), Tag::IntArray(vec![1, -2])),
            ("la".into(), Tag::LongArray(vec![i64::MAX])),
            ("ba".into(), Tag::ByteArray(vec![1, -1])),
            ("d".into(), Tag::Double(1.5)),
        ]);
        let mut buf = vec![];
        write(&mut buf, "", &root).unwrap();
        let (name, back) = read(&buf[..]).unwrap();
        assert_eq!(name, "");
        assert_eq!(back, root);
        let mut buf2 = vec![];
        write(&mut buf2, "", &back).unwrap();
        assert_eq!(buf, buf2, "bytes idênticos");
    }

    #[test]
    fn rejects_garbage() {
        assert!(read(&[1u8, 2, 3][..]).is_err());
        assert!(read(&[10u8, 0, 0, 99][..]).is_err());
    }
}
