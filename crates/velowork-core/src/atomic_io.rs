//! 原子文件写入工具。
//!
//! 流程：写入 `.tmp` 临时文件 → fsync → 设权限 0o600 → rename 替换。
//! 保证断电/崩溃不会产生半写文件。

use std::path::Path;

use anyhow::{Context, Result};

/// 原子写入任意字节内容到 `path`。
///
/// 1. 写入 `<path>.tmp` 临时文件
/// 2. `fsync` 确保数据落盘
/// 3. Unix 下设置 `0o600` 权限
/// 4. `rename` 替换目标文件
pub fn write_atomic(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating parent dir for {}", path.display()))?;
    }
    let tmp_path = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp_path)
            .with_context(|| format!("creating tmp file {}", tmp_path.display()))?;
        f.write_all(content)?;
        f.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp_path, path)
        .with_context(|| format!("renaming {} to {}", tmp_path.display(), path.display()))?;
    Ok(())
}

/// 原子写入 JSON Value 到 `path`（pretty-print 格式）。
pub fn write_json_atomic(path: &Path, value: &serde_json::Value) -> Result<()> {
    let content = serde_json::to_string_pretty(value)
        .context("serializing JSON for atomic write")?;
    write_atomic(path, content.as_bytes())
}

/// 原子写入字符串内容到 `path`（用于已序列化好的 JSON 字符串等场景）。
pub fn write_string_atomic(path: &Path, content: &str) -> Result<()> {
    write_atomic(path, content.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");
        write_atomic(&path, b"{}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
    }

    #[test]
    fn write_atomic_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("test.json");
        write_atomic(&path, b"hello").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn write_json_atomic_pretty_prints() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");
        let value = serde_json::json!({"key": "value"});
        write_json_atomic(&path, &value).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("\n")); // pretty-printed
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["key"], "value");
    }

    #[test]
    fn write_atomic_no_leftover_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");
        write_atomic(&path, b"{}").unwrap();
        let tmp_path = path.with_extension("tmp");
        assert!(!tmp_path.exists(), "tmp file should be cleaned up by rename");
    }
}
