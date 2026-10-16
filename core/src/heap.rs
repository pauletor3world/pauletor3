//! Memória do servidor conforme a RAM do PC.

/// Heap automático em MB a partir da RAM total em MB.
/// 8 GB → 3 GB; 16 GB → 5 GB; 32 GB+ → 6 GB. Nunca mais que metade da RAM.
pub fn auto_heap_mb(total_ram_mb: u64) -> u64 {
    let gb = total_ram_mb as f64 / 1024.0;
    let heap_gb: u64 = if gb < 6.0 {
        2
    } else if gb < 10.0 {
        3
    } else if gb < 14.0 {
        4
    } else if gb < 24.0 {
        5
    } else {
        6
    };
    (heap_gb * 1024).min((total_ram_mb / 2).max(1024))
}

pub fn total_ram_mb() -> u64 {
    let mut s = sysinfo::System::new();
    s.refresh_memory();
    s.total_memory() / (1024 * 1024)
}

/// Heap efetivo: o manual (se definido e sensato) ou o automático.
pub fn effective_heap_mb(manual: Option<u64>) -> u64 {
    let total = total_ram_mb();
    match manual {
        Some(m) if m >= 1024 && m <= total.saturating_sub(1024) => m,
        _ => auto_heap_mb(total),
    }
}

/// Argumentos da JVM: heap fixo (Xms = Xmx) e G1GC com ajustes conservadores.
pub fn jvm_args(heap_mb: u64) -> Vec<String> {
    vec![
        format!("-Xms{heap_mb}M"),
        format!("-Xmx{heap_mb}M"),
        "-XX:+UseG1GC".into(),
        "-XX:+ParallelRefProcEnabled".into(),
        "-XX:MaxGCPauseMillis=200".into(),
        "-XX:+DisableExplicitGC".into(),
        "-XX:+AlwaysPreTouch".into(),
        "-Dfile.encoding=UTF-8".into(),
        "-Dstdout.encoding=UTF-8".into(),
        "-Dstderr.encoding=UTF-8".into(),
    ]
}

/// Memória automática do cliente (o jogo). Se este PC também hospeda, o servidor e o sistema
/// vêm primeiro: o cliente fica com o que sobra, sem passar da tabela nem cair abaixo de 1 GB.
pub fn auto_client_heap_mb(total_ram_mb: u64, server_heap_mb: Option<u64>) -> u64 {
    let gb = total_ram_mb as f64 / 1024.0;
    let want: u64 = if gb < 6.0 {
        1536
    } else if gb < 10.0 {
        2048
    } else if gb < 20.0 {
        3072
    } else {
        4096
    };
    let room = total_ram_mb.saturating_sub(1536 + server_heap_mb.unwrap_or(0));
    want.min(room).max(1024)
}

/// Memória efetiva do cliente: a escolhida (se couber) ou a automática.
pub fn effective_client_heap_mb(manual: Option<u64>, server_heap_mb: Option<u64>) -> u64 {
    let total = total_ram_mb();
    match manual {
        Some(m) if m >= 1024 && m <= total.saturating_sub(1024) => m,
        _ => auto_client_heap_mb(total, server_heap_mb),
    }
}

/// Argumentos de memória do cliente (os mesmos ajustes de G1 do launcher oficial).
pub fn client_jvm_args(heap_mb: u64) -> Vec<String> {
    vec![
        format!("-Xms{}M", heap_mb.min(1024)),
        format!("-Xmx{heap_mb}M"),
        "-XX:+UnlockExperimentalVMOptions".into(),
        "-XX:+UseG1GC".into(),
        "-XX:G1NewSizePercent=20".into(),
        "-XX:G1ReservePercent=20".into(),
        "-XX:MaxGCPauseMillis=50".into(),
        "-XX:G1HeapRegionSize=32M".into(),
        "-Dstdout.encoding=UTF-8".into(),
        "-Dstderr.encoding=UTF-8".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_heap_leaves_room_for_server() {
        assert_eq!(auto_client_heap_mb(16 * 1024, None), 3072);
        assert_eq!(auto_client_heap_mb(32 * 1024, None), 4096);
        assert_eq!(auto_client_heap_mb(8 * 1024, None), 2048);
        // 8 GB hospedando (servidor com 3 GB): sobra 3,5 GB, o cliente fica com 2 GB
        assert_eq!(auto_client_heap_mb(8 * 1024, Some(3072)), 2048);
        // 6 GB hospedando: aperta, mas nunca abaixo de 1 GB
        assert_eq!(auto_client_heap_mb(6 * 1024, Some(3072)), 1536);
        assert_eq!(auto_client_heap_mb(4 * 1024, Some(2048)), 1024);
    }

    #[test]
    fn heap_table() {
        assert_eq!(auto_heap_mb(16 * 1024), 5 * 1024);
        assert_eq!(auto_heap_mb(8 * 1024), 3 * 1024);
        assert_eq!(auto_heap_mb(32 * 1024), 6 * 1024);
        assert_eq!(auto_heap_mb(4 * 1024), 2 * 1024);
        assert_eq!(auto_heap_mb(3 * 1024), 1536, "nunca mais que metade da RAM");
        assert!(jvm_args(3072).contains(&"-XX:+UseG1GC".to_string()));
    }
}
