//! Remote via rclone (sidecar). O remote padrão é o Google Drive, mas qualquer backend do
//! rclone funciona; os testes de integração usam o backend `local`.
//!
//! Comportamentos do rclone v1.75 conferidos na prática (ver docs/decisoes.md):
//! - `cat` de arquivo inexistente sai com código 3 (diretório não encontrado) ou 4 (objeto não encontrado);
//! - `lsjson` de pasta inexistente sai com 3;
//! - `deletefile` de arquivo inexistente sai com 4;
//! - com `--use-json-log --stats-log-level NOTICE`, cada linha de estatística traz um objeto
//!   `stats` com `bytes`, `totalBytes`, `transfers` e `totalTransfers`;
//! - a config pode ficar criptografada em disco, com a senha passada em `RCLONE_CONFIG_PASS`.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use async_trait::async_trait;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use zeroize::Zeroizing;

use super::{ProgressFn, Remote, RemoteEntry, TransferProgress};
use crate::{util, NmError, NmResult};

#[derive(Clone)]
pub struct RcloneRemote {
    bin: PathBuf,
    /// Arquivo de config do rclone (criptografado). `None` = só backends "on the fly" (`:local:`).
    config: Option<PathBuf>,
    config_pass: Option<Zeroizing<String>>,
    /// Ex.: `pauletor3:pAuletOr3` ou `:local:D:/teste/remote`.
    base: String,
    tmp: PathBuf,
    pub transfers: u32,
}

/// Nome do remote dentro da config do rclone.
pub const REMOTE_NAME: &str = "pauletor3";

impl RcloneRemote {
    pub fn new(bin: PathBuf, config: PathBuf, config_pass: Zeroizing<String>, root_folder: &str, tmp: PathBuf) -> Self {
        Self {
            bin,
            config: Some(config),
            config_pass: Some(config_pass),
            base: format!("{REMOTE_NAME}:{}", root_folder.trim_matches('/')),
            tmp,
            transfers: 4,
        }
    }

    /// Remote numa pasta local, via backend `local` do rclone (testes e emergência).
    pub fn local(bin: PathBuf, dir: &Path, tmp: PathBuf) -> Self {
        let d = dir.to_string_lossy().replace('\\', "/");
        Self { bin, config: None, config_pass: None, base: format!(":local:{d}"), tmp, transfers: 4 }
    }

    fn target(&self, rel: &str) -> String {
        if rel.is_empty() {
            self.base.clone()
        } else {
            format!("{}/{}", self.base, rel.trim_start_matches('/'))
        }
    }

    fn cmd(&self) -> tokio::process::Command {
        let mut c = util::command(&self.bin);
        match &self.config {
            Some(cfg) => {
                c.arg("--config").arg(cfg);
            }
            None => {
                // Nunca tocar na config do rclone do usuário.
                c.arg("--config").arg(self.tmp.join("rclone-vazio.conf"));
            }
        }
        if let Some(p) = &self.config_pass {
            c.env("RCLONE_CONFIG_PASS", p.as_str());
        }
        c.arg("--ask-password=false");
        c.env_remove("RCLONE_CONFIG");
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        c
    }

    async fn run(&self, args: &[&str], input: Option<&[u8]>) -> NmResult<(i32, Vec<u8>, String)> {
        let mut c = self.cmd();
        c.args(args);
        if input.is_some() {
            c.stdin(Stdio::piped());
        }
        let mut child = c
            .spawn()
            .map_err(|e| NmError::Remote(format!("não consegui executar o rclone ({}): {e}", self.bin.display())))?;
        if let Some(data) = input {
            use tokio::io::AsyncWriteExt;
            let mut stdin = child.stdin.take().unwrap();
            stdin.write_all(data).await?;
            drop(stdin);
        }
        let out = child.wait_with_output().await?;
        let code = out.status.code().unwrap_or(-1);
        Ok((code, out.stdout, String::from_utf8_lossy(&out.stderr).into_owned()))
    }

    fn fail(what: &str, code: i32, stderr: &str) -> NmError {
        let last: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).rev().take(3).collect();
        let msg: Vec<&str> = last.into_iter().rev().collect();
        NmError::Remote(format!("{what} (código {code}): {}", msg.join(" | ")))
    }

    /// Roda uma transferência em lote acompanhando o progresso pelo log JSON do rclone.
    async fn transfer(&self, op_args: Vec<String>, names: &[String], progress: ProgressFn) -> NmResult<()> {
        if names.is_empty() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.tmp)?;
        let list = self.tmp.join(format!("files-{}.txt", uuid::Uuid::new_v4().simple()));
        std::fs::write(&list, names.join("\n") + "\n")?;
        let transfers = self.transfers.to_string();
        let checkers = (self.transfers * 2).to_string();
        let mut c = self.cmd();
        c.args(&op_args)
            .arg("--files-from-raw")
            .arg(&list)
            .args([
                "--no-traverse",
                "--transfers",
                &transfers,
                "--checkers",
                &checkers,
                "--retries",
                "3",
                "--low-level-retries",
                "10",
                "--use-json-log",
                "--stats",
                "500ms",
                "--stats-log-level",
                "NOTICE",
            ]);
        let mut child = c.spawn().map_err(|e| NmError::Remote(format!("rclone: {e}")))?;
        let stderr = child.stderr.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let reader = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut errors = Vec::new();
            while let Ok(Some(line)) = lines.next_line().await {
                match serde_json::from_str::<LogLine>(&line) {
                    Ok(l) => {
                        if let Some(s) = l.stats {
                            progress(TransferProgress {
                                bytes_done: s.bytes,
                                bytes_total: s.total_bytes,
                                files_done: s.transfers,
                                files_total: s.total_transfers,
                            });
                        } else if l.level.as_deref() == Some("error") || l.level.as_deref() == Some("critical") {
                            errors.push(l.msg.unwrap_or_default());
                        }
                    }
                    Err(_) => errors.push(line),
                }
            }
            errors
        });
        let mut sink = Vec::new();
        let _ = stdout.read_to_end(&mut sink).await;
        let status = child.wait().await?;
        let errors = reader.await.unwrap_or_default();
        let _ = std::fs::remove_file(&list);
        if !status.success() {
            let tail: Vec<String> = errors.into_iter().rev().take(3).collect();
            return Err(NmError::Remote(format!(
                "transferência falhou (código {}): {}",
                status.code().unwrap_or(-1),
                tail.join(" | ")
            )));
        }
        Ok(())
    }

    /// Testa acesso ao remote (lista a raiz).
    pub async fn check(&self) -> NmResult<()> {
        let (code, _, err) = self.run(&["lsjson", "--max-depth", "1", "--files-only", &self.target("")], None).await?;
        if code == 0 || code == 3 {
            Ok(())
        } else {
            Err(Self::fail("remote inacessível", code, &err))
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }
}

#[derive(Deserialize)]
struct LogLine {
    level: Option<String>,
    msg: Option<String>,
    stats: Option<Stats>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stats {
    #[serde(default)]
    bytes: u64,
    #[serde(default)]
    total_bytes: u64,
    #[serde(default)]
    transfers: u64,
    #[serde(default)]
    total_transfers: u64,
}

#[derive(Deserialize)]
struct LsEntry {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Size")]
    size: i64,
    #[serde(rename = "IsDir")]
    is_dir: bool,
}

#[async_trait]
impl Remote for RcloneRemote {
    async fn read(&self, path: &str) -> NmResult<Option<Vec<u8>>> {
        let (code, out, err) = self.run(&["cat", &self.target(path)], None).await?;
        match code {
            0 if out.is_empty() => Ok(None),
            0 => Ok(Some(out)),
            3 | 4 => Ok(None),
            _ => Err(Self::fail(&format!("lendo {path}"), code, &err)),
        }
    }

    async fn write(&self, path: &str, data: &[u8]) -> NmResult<()> {
        // copyto a partir de um temporário: atualiza o arquivo existente em vez de criar outro
        // com o mesmo nome (no Drive, nomes repetidos são permitidos).
        std::fs::create_dir_all(&self.tmp)?;
        let tmp = self.tmp.join(format!("up-{}", uuid::Uuid::new_v4().simple()));
        std::fs::write(&tmp, data)?;
        let res = self
            .run(&["copyto", "--ignore-times", "--retries", "3", &tmp.to_string_lossy(), &self.target(path)], None)
            .await;
        let _ = std::fs::remove_file(&tmp);
        let (code, _, err) = res?;
        if code != 0 {
            return Err(Self::fail(&format!("gravando {path}"), code, &err));
        }
        Ok(())
    }

    async fn delete(&self, path: &str) -> NmResult<()> {
        let (code, _, err) = self.run(&["deletefile", "--retries", "1", &self.target(path)], None).await?;
        match code {
            0 | 3 | 4 => Ok(()),
            _ => Err(Self::fail(&format!("apagando {path}"), code, &err)),
        }
    }

    async fn list(&self, dir: &str) -> NmResult<Vec<RemoteEntry>> {
        let (code, out, err) = self
            .run(&["lsjson", "--files-only", "--no-mimetype", "--no-modtime", &self.target(dir)], None)
            .await?;
        match code {
            0 => {
                let v: Vec<LsEntry> = serde_json::from_slice(&out)
                    .map_err(|e| NmError::Remote(format!("lsjson ilegível: {e}")))?;
                let mut v: Vec<RemoteEntry> = v
                    .into_iter()
                    .filter(|e| !e.is_dir && !e.name.starts_with('.'))
                    .map(|e| RemoteEntry { name: e.name, size: e.size.max(0) as u64 })
                    .collect();
                v.sort_by(|a, b| a.name.cmp(&b.name));
                Ok(v)
            }
            3 => Ok(vec![]),
            _ => Err(Self::fail(&format!("listando {dir}"), code, &err)),
        }
    }

    async fn upload_many(&self, local_dir: &Path, names: &[String], remote_dir: &str, progress: ProgressFn) -> NmResult<()> {
        let args = vec!["copy".into(), local_dir.to_string_lossy().into_owned(), self.target(remote_dir)];
        self.transfer(args, names, progress).await
    }

    async fn download_many(&self, remote_dir: &str, names: &[String], local_dir: &Path, progress: ProgressFn) -> NmResult<()> {
        std::fs::create_dir_all(local_dir)?;
        let args = vec!["copy".into(), self.target(remote_dir), local_dir.to_string_lossy().into_owned()];
        self.transfer(args, names, progress).await
    }

    async fn delete_many(&self, dir: &str, names: &[String]) -> NmResult<()> {
        if names.is_empty() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.tmp)?;
        let list = self.tmp.join(format!("del-{}.txt", uuid::Uuid::new_v4().simple()));
        std::fs::write(&list, names.join("\n") + "\n")?;
        let res = self
            .run(&["delete", "--files-from-raw", &list.to_string_lossy(), &self.target(dir)], None)
            .await;
        let _ = std::fs::remove_file(&list);
        let (code, _, err) = res?;
        match code {
            0 | 3 => Ok(()),
            _ => Err(Self::fail(&format!("apagando em {dir}"), code, &err)),
        }
    }

    fn describe(&self) -> String {
        format!("rclone {}", self.base)
    }
}

/// Operações de configuração (feitas uma vez, no assistente).
pub mod setup {
    use super::*;

    /// Cria uma config criptografada contendo o remote `pauletor3` com os parâmetros dados.
    /// O arquivo nunca existe em texto puro com segredo dentro: primeiro é criado com um
    /// remote vazio, criptografado, e só então o remote real é gravado.
    pub async fn create_encrypted_config(
        bin: &Path,
        config: &Path,
        pass: &str,
        backend: &str,
        params: &[(String, String)],
    ) -> NmResult<()> {
        if let Some(d) = config.parent() {
            std::fs::create_dir_all(d)?;
        }
        let _ = std::fs::remove_file(config);
        let run = |args: Vec<String>, with_pass: bool| {
            let mut c = util::command(bin);
            c.arg("--config").arg(config).arg("--ask-password=false").args(args);
            c.env_remove("RCLONE_CONFIG");
            if with_pass {
                c.env("RCLONE_CONFIG_PASS", pass);
            }
            c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
            c
        };
        let check = |o: std::process::Output, what: &str| -> NmResult<()> {
            if o.status.success() {
                Ok(())
            } else {
                Err(NmError::Remote(format!("{what}: {}", String::from_utf8_lossy(&o.stderr).trim())))
            }
        };
        // 1) config só com um remote inofensivo
        let o = run(vec!["config".into(), "create".into(), "nm-vazio".into(), "local".into()], false)
            .output()
            .await?;
        check(o, "criando config")?;
        // 2) criptografa (senha lida pelo --password-command, sem prompt)
        let mut c = run(vec!["config".into(), "encryption".into(), "set".into()], false);
        c.arg("--password-command").arg(password_command(pass));
        check(c.output().await?, "criptografando config")?;
        // 3) grava o remote real (já criptografado)
        let mut args = vec!["config".into(), "create".into(), REMOTE_NAME.into(), backend.into()];
        for (k, v) in params {
            args.push(format!("{k}={v}"));
        }
        args.push("--non-interactive".into());
        let o = run(args, true).output().await?;
        check(o, "gravando remote")?;
        let o = run(vec!["config".into(), "delete".into(), "nm-vazio".into()], true).output().await?;
        check(o, "limpando config")?;
        Ok(())
    }

    /// Comando que imprime a senha (o rclone o executa para ler a senha de criptografia).
    fn password_command(pass: &str) -> String {
        // A senha é gerada por nós (alfanumérica), então não há o que escapar.
        debug_assert!(pass.chars().all(|c| c.is_ascii_alphanumeric()));
        if cfg!(windows) {
            format!("cmd /c echo {pass}")
        } else {
            format!("echo {pass}")
        }
    }

    /// Roda `rclone authorize drive [client_id client_secret]`, que abre o navegador para o login
    /// do Google, e devolve o token JSON que o rclone imprime entre `--->` e `<---End paste`.
    ///
    /// O client ID compartilhado do rclone está sendo aposentado em 2026 (aviso do próprio
    /// rclone v1.75), por isso o app usa o client ID do dono quando informado.
    pub async fn authorize_drive(bin: &Path, client: Option<(&str, &str)>) -> NmResult<String> {
        let mut c = util::command(bin);
        c.args(["authorize", "drive"]).env_remove("RCLONE_CONFIG");
        if let Some((id, secret)) = client {
            c.arg(id).arg(secret);
        }
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let out = c.output().await?;
        // O texto com o token pode sair em stdout ou stderr, conforme a versão.
        let text = format!("{}
{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        if !out.status.success() {
            return Err(NmError::Remote(format!(
                "autorização do Google não concluída: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        extract_token(&text).ok_or_else(|| NmError::Remote("o rclone não devolveu um token".into()))
    }

    pub fn extract_token(text: &str) -> Option<String> {
        let start = text.find("--->")? + 4;
        let end = text[start..].find("<---")? + start;
        let tok = text[start..end].trim();
        serde_json::from_str::<serde_json::Value>(tok).ok()?;
        Some(tok.to_string())
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn extracts_token() {
            let t = "Paste the following into your remote machine --->\n{\"access_token\":\"x\",\"expiry\":\"y\"}\n<---End paste\n";
            assert_eq!(super::extract_token(t).unwrap(), "{\"access_token\":\"x\",\"expiry\":\"y\"}");
            assert!(super::extract_token("nada").is_none());
        }
    }
}
