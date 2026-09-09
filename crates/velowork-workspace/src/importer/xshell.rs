use anyhow::{Context, Result, bail};
use std::io::Read;
use std::path::Path;
use velowork_state::{SessionProtocol, SshAuthType};

use super::common::{decode_bytes, parse_ini_sections};
use super::{ImportContext, ImportedSession, SessionImporter};

pub struct XshellImporter;

impl SessionImporter for XshellImporter {
    fn id(&self) -> &'static str {
        "xshell"
    }

    fn display_name(&self) -> &'static str {
        "Xshell (.xts, .xsh)"
    }

    fn file_extensions(&self) -> &'static [&'static str] {
        &[".xts", ".xsh"]
    }

    fn parse(&self, ctx: &ImportContext) -> Result<Vec<ImportedSession>> {
        let path = &ctx.source_path;
        if !path.exists() {
            bail!("File not found: {}", path.display());
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ext == "xts" || ext == "zip" {
            parse_xshell_archive(path)
        } else if ext == "xsh" {
            parse_single_xsh(path)
        } else {
            // Try archive first, then fallback to single xsh
            if let Ok(sessions) = parse_xshell_archive(path) {
                Ok(sessions)
            } else {
                parse_single_xsh(path)
            }
        }
    }
}

fn parse_xshell_archive(path: &Path) -> Result<Vec<ImportedSession>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Cannot open archive: {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("Invalid ZIP/XTS file: {}", path.display()))?;

    let mut sessions = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .with_context(|| format!("Failed to read entry #{i}"))?;

        // ZIP filenames on Chinese Windows are often GBK-encoded
        let entry_path_raw = entry.name_raw().to_vec();
        let entry_path = decode_bytes(&entry_path_raw);
        if !entry_path.ends_with(".xsh") {
            continue;
        }

        let mut raw = Vec::new();
        entry
            .read_to_end(&mut raw)
            .with_context(|| format!("Failed to read content for {entry_path}"))?;

        let content = decode_bytes(&raw);
        if let Some(sess) = parse_xsh_content(&content, &entry_path) {
            sessions.push(sess);
        }
    }

    Ok(sessions)
}

fn parse_single_xsh(path: &Path) -> Result<Vec<ImportedSession>> {
    let raw = std::fs::read(path)
        .with_context(|| format!("Cannot read file: {}", path.display()))?;
    let content = decode_bytes(&raw);
    let path_str = path.to_string_lossy();

    if let Some(sess) = parse_xsh_content(&content, &path_str) {
        Ok(vec![sess])
    } else {
        Ok(Vec::new())
    }
}

pub fn parse_xsh_content(content: &str, entry_path: &str) -> Option<ImportedSession> {
    let sections = parse_ini_sections(content);

    let conn = sections.get("CONNECTION")?;
    let protocol_str = conn.get("Protocol").map(String::as_str).unwrap_or("");
    if !protocol_str.is_empty() && !protocol_str.eq_ignore_ascii_case("SSH") {
        return None;
    }

    let host = conn.get("Host")?.trim().to_string();
    if host.is_empty() {
        return None;
    }

    let port: u16 = conn.get("Port").and_then(|p| p.parse().ok()).unwrap_or(22);

    let auth = sections.get("CONNECTION:AUTHENTICATION");
    let username = auth
        .and_then(|a| a.get("UserName"))
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "root".to_string());

    let user_key = auth
        .and_then(|a| a.get("UserKey"))
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());

    let auth_type = if let Some(key_name) = user_key {
        SshAuthType::PrivateKey {
            key_path: key_name,
            passphrase: None,
        }
    } else {
        SshAuthType::Password { password: None }
    };

    let path_obj = Path::new(entry_path);
    let name = path_obj
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Unnamed")
        .to_string();

    let group_path = path_obj.parent().and_then(|p| {
        let p_str = p.to_string_lossy().replace('\\', "/");
        let stripped = p_str
            .strip_prefix("Xshell/Sessions/")
            .or_else(|| p_str.strip_prefix("Xshell/"))
            .or_else(|| p_str.strip_prefix("Sessions/"))
            .unwrap_or(&p_str);

        if stripped.is_empty() {
            None
        } else {
            let segments: Vec<String> = stripped
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            if segments.is_empty() {
                None
            } else {
                Some(segments)
            }
        }
    });

    Some(ImportedSession {
        name,
        protocol: SessionProtocol::Ssh,
        host,
        port,
        username,
        auth_type,
        group_path,
        description: None,
    })
}
