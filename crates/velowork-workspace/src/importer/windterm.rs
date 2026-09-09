use aes::Aes256;
use anyhow::{Context, Result, bail};
use base64::Engine;
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use pbkdf2::pbkdf2_hmac;
use sha3::Sha3_512;
use std::path::{Path, PathBuf};
use velowork_state::{SessionProtocol, SshAuthType};

use super::{ImportContext, ImportedSession, SessionImporter};

type Aes256CbcDecryptor = cbc::Decryptor<Aes256>;

const WINDTERM_PBKDF2_ITERATIONS: u32 = 100_000;
const WINDTERM_DERIVED_LENGTH: usize = 48;
const WINDTERM_AES_KEY_LENGTH: usize = 32;
const WINDTERM_AES_IV_LENGTH: usize = 16;

pub struct WindtermCrypto {
    key: [u8; WINDTERM_AES_KEY_LENGTH],
    iv: [u8; WINDTERM_AES_IV_LENGTH],
}

pub struct WindTermImporter;

impl SessionImporter for WindTermImporter {
    fn id(&self) -> &'static str {
        "windterm"
    }

    fn display_name(&self) -> &'static str {
        "WindTerm (.sessions)"
    }

    fn file_extensions(&self) -> &'static [&'static str] {
        &[".sessions", ".json"]
    }

    fn parse(&self, ctx: &ImportContext) -> Result<Vec<ImportedSession>> {
        let path = &ctx.source_path;
        if !path.exists() {
            bail!("指定的文件或路径不存在: {}", path.display());
        }

        if path.is_file() {
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("无法读取 WindTerm 文件: {}", path.display()))?;

            let crypto = load_windterm_crypto(path, ctx.master_password.as_deref())?;

            parse_windterm_content(&content, crypto.as_ref(), Some(path))
        } else if path.is_dir() {
            let crypto = load_windterm_crypto(path, ctx.master_password.as_deref())?;
            let mut all_sessions = Vec::new();
            let mut files = Vec::new();
            collect_files_recursive(path, 4, &mut files);

            for file_path in files {
                let file_name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if file_name.ends_with(".sessions")
                    || file_name.ends_with(".json")
                    || file_name == "user.sessions"
                {
                    if let Ok(content) = std::fs::read_to_string(&file_path) {
                        if let Ok(mut sessions) =
                            parse_windterm_content(&content, crypto.as_ref(), Some(&file_path))
                        {
                            all_sessions.append(&mut sessions);
                        }
                    }
                }
            }

            if all_sessions.is_empty() {
                bail!("在目录 {} 中未找到有效的 WindTerm 会话文件", path.display());
            }

            Ok(all_sessions)
        } else {
            bail!("不受支持的文件类型: {}", path.display());
        }
    }
}

pub fn parse_windterm_content(
    content: &str,
    crypto: Option<&WindtermCrypto>,
    source_path: Option<&Path>,
) -> Result<Vec<ImportedSession>> {
    let parsed: serde_json::Value =
        serde_json::from_str(content).with_context(|| "WindTerm 会话文件不是有效的 JSON 格式")?;

    let entries: Vec<serde_json::Value> = match parsed {
        serde_json::Value::Array(arr) => arr,
        serde_json::Value::Object(obj) => {
            if let Some(serde_json::Value::Array(arr)) = obj.get("sessions") {
                arr.clone()
            } else {
                vec![serde_json::Value::Object(obj)]
            }
        }
        _ => bail!("WindTerm 会话文件根节点必须是会话数组或对象"),
    };

    let mut sessions = Vec::new();

    for entry in &entries {
        let protocol = entry
            .get("session.protocol")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !protocol.is_empty() && !protocol.eq_ignore_ascii_case("SSH") {
            continue;
        }

        let target = entry
            .get("session.target")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let (host, target_username) = parse_windterm_target(target);
        if host.is_empty() {
            continue;
        }

        let name = entry
            .get("session.label")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&host)
            .to_string();

        let port = match entry.get("session.port").and_then(|v| v.as_u64()) {
            Some(p) if (1..=u64::from(u16::MAX)).contains(&p) => p as u16,
            Some(_) => continue,
            None => 22,
        };

        let auto_login = parse_windterm_auto_login(entry, crypto)?;

        let username = auto_login
            .as_ref()
            .and_then(|payload| payload.get("session.user"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or(target_username);

        let group_path = parse_windterm_group_path(entry);

        let description = entry
            .get("session.description")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let auth_type = extract_windterm_auth(entry, auto_login.as_ref(), source_path)?;

        sessions.push(ImportedSession {
            name,
            protocol: SessionProtocol::Ssh,
            host,
            port,
            username,
            auth_type,
            group_path,
            description,
        });
    }

    Ok(sessions)
}

fn parse_windterm_target(target: &str) -> (String, String) {
    let target = target.trim();
    if let Some((username, host)) = target.rsplit_once('@') {
        if !username.is_empty() && !host.is_empty() {
            return (host.to_string(), username.to_string());
        }
    }
    (target.to_string(), "root".to_string())
}

fn collect_files_recursive(dir: &Path, max_depth: usize, out: &mut Vec<PathBuf>) {
    if max_depth == 0 {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_file() {
                out.push(path);
            } else if path.is_dir() {
                collect_files_recursive(&path, max_depth - 1, out);
            }
        }
    }
}

fn find_file_in_dir_recursive(dir: &Path, file_name: &str, max_depth: usize) -> Option<PathBuf> {
    if max_depth == 0 {
        return None;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_file() {
                if path.file_name().and_then(|n| n.to_str()) == Some(file_name) {
                    return Some(path);
                }
            } else if path.is_dir() {
                if let Some(found) = find_file_in_dir_recursive(&path, file_name, max_depth - 1) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn find_windterm_user_config(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        if path.file_name().map_or(false, |f| f == "user.config") {
            return Some(path.to_path_buf());
        }
        if let Some(parent) = path.parent() {
            let candidate = parent.join("user.config");
            if candidate.is_file() {
                return Some(candidate);
            }
            if let Some(grandparent) = parent.parent() {
                let candidate = grandparent.join("user.config");
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    } else if path.is_dir() {
        let candidate = path.join("user.config");
        if candidate.is_file() {
            return Some(candidate);
        }
        if let Some(found) = find_file_in_dir_recursive(path, "user.config", 3) {
            return Some(found);
        }
    }
    None
}

fn load_windterm_crypto(
    sessions_path: &Path,
    windterm_master_password: Option<&str>,
) -> Result<Option<WindtermCrypto>> {
    let Some(config_path) = find_windterm_user_config(sessions_path) else {
        if let Some(pwd) = windterm_master_password {
            if !pwd.trim().is_empty() {
                bail!(
                    "未在同级或父级目录中找到 WindTerm 的 user.config 配置文件（缺少 application.fingerprint 设备指纹）。请将 user.config 与会话文件放置在同一配置目录下后重试。"
                );
            }
        }
        return Ok(None);
    };

    let content = std::fs::read_to_string(&config_path).with_context(|| {
        format!(
            "无法读取 WindTerm user.config 配置文件: {}",
            config_path.display()
        )
    })?;
    let config: serde_json::Value = serde_json::from_str(&content).with_context(|| {
        format!(
            "WindTerm user.config 不是有效的 JSON 格式: {}",
            config_path.display()
        )
    })?;

    let fingerprint = config
        .get("application.fingerprint")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "WindTerm user.config 配置文件缺少 application.fingerprint 设备指纹字段"
            )
        })?;

    let master_password_enabled = config
        .get("application.masterPassword")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    let pwd = windterm_master_password.map(str::trim).unwrap_or("");
    if master_password_enabled && pwd.is_empty() {
        bail!("该 WindTerm 配置文件已启用密码保护，请在密码框输入正确的密码后再导入。");
    }

    Ok(Some(derive_windterm_crypto(fingerprint, pwd)))
}

pub fn derive_windterm_crypto(fingerprint: &str, master_password: &str) -> WindtermCrypto {
    let mut material = [0_u8; WINDTERM_DERIVED_LENGTH];
    pbkdf2_hmac::<Sha3_512>(
        master_password.as_bytes(),
        fingerprint.as_bytes(),
        WINDTERM_PBKDF2_ITERATIONS,
        &mut material,
    );

    let mut key = [0_u8; WINDTERM_AES_KEY_LENGTH];
    key.copy_from_slice(&material[..WINDTERM_AES_KEY_LENGTH]);
    let mut iv = [0_u8; WINDTERM_AES_IV_LENGTH];
    iv.copy_from_slice(&material[WINDTERM_AES_KEY_LENGTH..]);
    WindtermCrypto { key, iv }
}

fn parse_windterm_auto_login(
    entry: &serde_json::Value,
    crypto: Option<&WindtermCrypto>,
) -> Result<Option<serde_json::Map<String, serde_json::Value>>> {
    let Some(raw) = entry
        .get("session.autoLogin")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) {
        return value
            .as_object()
            .cloned()
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("WindTerm session.autoLogin 内容不是 JSON 对象"));
    }

    let crypto = crypto.ok_or_else(|| {
        anyhow::anyhow!("检测到 WindTerm 会话登录凭据已加密，但未找到 user.config 配置文件（缺少 application.fingerprint 设备指纹）。请确保 user.config 位于同级或父级目录中。")
    })?;

    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| anyhow::anyhow!("WindTerm autoLogin 凭据 Base64 解码失败"))?;

    let plaintext = decrypt_windterm_auto_login(&ciphertext, crypto)?;
    let plaintext_str = String::from_utf8(plaintext)
        .map_err(|_| anyhow::anyhow!("WindTerm 凭据解密结果非有效 UTF-8 文本（可能密码不正确）"))?;

    let value: serde_json::Value = serde_json::from_str(&plaintext_str).map_err(|_| {
        anyhow::anyhow!("解密后的 WindTerm 凭据不是有效的 JSON 数据（可能密码不正确）")
    })?;

    value
        .as_object()
        .cloned()
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("解密后的 WindTerm 凭据不是 JSON 对象"))
}

fn decrypt_windterm_auto_login(ciphertext: &[u8], crypto: &WindtermCrypto) -> Result<Vec<u8>> {
    if ciphertext.is_empty() || ciphertext.len() % WINDTERM_AES_IV_LENGTH != 0 {
        bail!("WindTerm 凭据密文长度无效（未按 16 字节对齐）");
    }

    let mut buffer = ciphertext.to_vec();
    Aes256CbcDecryptor::new(&crypto.key.into(), &crypto.iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buffer)
        .map(|plaintext| plaintext.to_vec())
        .map_err(|_| anyhow::anyhow!("密码错误或设备指纹不匹配，解密失败"))
}

fn parse_windterm_group_path(entry: &serde_json::Value) -> Option<Vec<String>> {
    entry
        .get("session.group")
        .and_then(|v| v.as_str())
        .and_then(|s| {
            let segments: Vec<String> = s
                .split('>')
                .map(str::trim)
                .filter(|seg| !seg.is_empty())
                .map(str::to_string)
                .collect();
            if segments.is_empty() {
                None
            } else {
                Some(segments)
            }
        })
}

fn extract_windterm_auth(
    entry: &serde_json::Value,
    auto_login: Option<&serde_json::Map<String, serde_json::Value>>,
    source_path: Option<&Path>,
) -> Result<SshAuthType> {
    // 1. Password in autoLogin
    if let Some(password) = auto_login.and_then(|payload| {
        let enabled = payload
            .get("PasswordEnabled")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if enabled {
            payload
                .get("Password")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
        } else {
            None
        }
    }) {
        return Ok(SshAuthType::Password {
            password: Some(password.to_string()),
        });
    }

    // 2. Private Key in autoLogin or session
    let mut key_paths = Vec::new();
    if let Some(path) = auto_login
        .and_then(|payload| payload.get("Public Key"))
        .and_then(|value| value.as_object())
        .and_then(|object| object.get("windows.path"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        key_paths.push(path);
    }

    if let Some(path) = entry
        .get("ssh.identityFilePath.windows")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !key_paths.contains(&path) {
            key_paths.push(path);
        }
    }

    if !key_paths.is_empty() {
        let passphrase = auto_login
            .and_then(|payload| payload.get("Public Key"))
            .and_then(|value| value.as_object())
            .and_then(|object| object.get("windows.pass"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let key_path = key_paths[0];
        let resolved = resolve_windterm_key_path(key_path, source_path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| key_path.to_string());

        return Ok(SshAuthType::PrivateKey {
            key_path: resolved,
            passphrase,
        });
    }

    Ok(SshAuthType::Password { password: None })
}

fn resolve_windterm_key_path(path: &str, source_path: Option<&Path>) -> Option<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }

    let home = dirs::home_dir();
    let mut expanded = trimmed.to_string();
    if let Some(home) = &home {
        let home_str = home.to_string_lossy();
        expanded = expanded
            .replace("$(HomeDir)", &home_str)
            .replace("${HomeDir}", &home_str);
        if expanded == "~" {
            expanded = home_str.to_string();
        } else if let Some(rest) = expanded
            .strip_prefix("~/")
            .or_else(|| expanded.strip_prefix("~\\"))
        {
            expanded = home.join(rest).to_string_lossy().to_string();
        }
    }

    let candidate = Path::new(&expanded);
    if candidate.is_absolute() {
        return Some(candidate.to_path_buf());
    }

    source_path
        .and_then(Path::parent)
        .map(|parent| parent.join(candidate))
        .or_else(|| Some(candidate.to_path_buf()))
}
