use anyhow::{Context, Result, bail};
use velowork_state::{SessionProtocol, SshAuthType};

use super::common::{decode_bytes, parse_ini_sections};
use super::{ImportContext, ImportedSession, SessionImporter};

pub struct MobaXtermImporter;

impl SessionImporter for MobaXtermImporter {
    fn id(&self) -> &'static str {
        "mobaxterm"
    }

    fn display_name(&self) -> &'static str {
        "MobaXterm (.mxtsessions)"
    }

    fn file_extensions(&self) -> &'static [&'static str] {
        &[".mxtsessions"]
    }

    fn parse(&self, ctx: &ImportContext) -> Result<Vec<ImportedSession>> {
        let path = &ctx.source_path;
        if !path.exists() {
            bail!("File not found: {}", path.display());
        }

        let raw = std::fs::read(path)
            .with_context(|| format!("Cannot read MobaXterm file: {}", path.display()))?;
        let content = decode_bytes(&raw);

        parse_mobaxterm_content(&content)
    }
}

pub fn parse_mobaxterm_content(content: &str) -> Result<Vec<ImportedSession>> {
    let sections = parse_ini_sections(content);
    let mut sessions = Vec::new();

    for (section_name, entries) in &sections {
        if !section_name.starts_with("Bookmarks") {
            continue;
        }

        let group_path = entries.get("SubRep").and_then(|s| {
            let s = s.trim();
            if s.is_empty() {
                None
            } else {
                let segments: Vec<String> = s
                    .split('\\')
                    .filter(|seg| !seg.is_empty())
                    .map(|seg| seg.trim().to_string())
                    .collect();
                if segments.is_empty() {
                    None
                } else {
                    Some(segments)
                }
            }
        });

        for (entry_name, value) in entries {
            if entry_name == "SubRep" || entry_name == "ImgNum" {
                continue;
            }

            if let Some(sess) = parse_moba_entry(entry_name, value, &group_path) {
                sessions.push(sess);
            }
        }
    }

    Ok(sessions)
}

fn parse_moba_entry(
    name: &str,
    value: &str,
    group_path: &Option<Vec<String>>,
) -> Option<ImportedSession> {
    // Format: #<type>#<subtype>%host%port%username%...
    let hash_parts: Vec<&str> = value.splitn(2, '#').skip(1).collect();
    if hash_parts.is_empty() {
        return None;
    }

    let after_hash = hash_parts.join("#");
    let type_and_rest: Vec<&str> = after_hash.splitn(2, '%').collect();
    if type_and_rest.len() < 2 {
        return None;
    }

    // Type 109 = SSH
    let type_marker = type_and_rest[0];
    if !type_marker.starts_with("109") {
        return None;
    }

    let fields: Vec<&str> = type_and_rest[1].split('%').collect();
    if fields.is_empty() {
        return None;
    }

    let host = fields[0].trim().to_string();
    if host.is_empty() {
        return None;
    }

    let port: u16 = fields
        .get(1)
        .and_then(|p| p.parse().ok())
        .filter(|p| *p > 0)
        .unwrap_or(22);

    let username = fields
        .get(2)
        .map(|u| u.trim())
        .filter(|u| !u.is_empty())
        .unwrap_or("root")
        .to_string();

    Some(ImportedSession {
        name: name.trim().to_string(),
        protocol: SessionProtocol::Ssh,
        host,
        port,
        username,
        auth_type: SshAuthType::Password { password: None },
        group_path: group_path.clone(),
        description: None,
    })
}
