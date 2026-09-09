//! Rolling file appender with size-based rotation and maximum file count retention.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Default maximum size for a single log file (10 MB).
pub const DEFAULT_MAX_LOG_SIZE_MB: u64 = 10;
pub const DEFAULT_MAX_LOG_SIZE_BYTES: u64 = DEFAULT_MAX_LOG_SIZE_MB * 1024 * 1024;

/// Default maximum number of rotated historical backup files to retain.
pub const DEFAULT_MAX_LOG_FILES: usize = 5;

/// Environment variable names for log rotation overrides.
pub const ENV_MAX_LOG_SIZE_MB: &str = "VELOWORK_LOG_MAX_SIZE_MB";
pub const ENV_MAX_LOG_FILES: &str = "VELOWORK_LOG_MAX_FILES";

/// Reads the maximum log size in bytes from the environment, falling back to [`DEFAULT_MAX_LOG_SIZE_BYTES`].
/// Valid range: 1 MB ..= 1024 MB.
pub fn max_log_size_bytes_from_env() -> u64 {
    if let Ok(val) = std::env::var(ENV_MAX_LOG_SIZE_MB)
        && let Ok(mb) = val.trim().parse::<u64>()
        && (1..=1024).contains(&mb)
    {
        return mb * 1024 * 1024;
    }
    DEFAULT_MAX_LOG_SIZE_BYTES
}

/// Reads the maximum number of backup files to retain from the environment, falling back to [`DEFAULT_MAX_LOG_FILES`].
/// Valid range: 1 ..= 50.
pub fn max_log_files_from_env() -> usize {
    if let Ok(val) = std::env::var(ENV_MAX_LOG_FILES)
        && let Ok(count) = val.trim().parse::<usize>()
        && (1..=50).contains(&count)
    {
        return count;
    }
    DEFAULT_MAX_LOG_FILES
}

/// Helper to generate numbered rotated log paths (e.g. `velowork.1.log`, `velowork.2.log`).
pub fn numbered_path(path: &Path, idx: usize) -> PathBuf {
    let parent = path.parent();
    let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("velowork");
    let ext = path.extension().and_then(|s| s.to_str());
    let new_name = match ext {
        Some(ext) => format!("{file_stem}.{idx}.{ext}"),
        None => format!("{file_stem}.{idx}"),
    };
    match parent {
        Some(p) => p.join(new_name),
        None => PathBuf::from(new_name),
    }
}

/// A rotating file writer that appends to a log file and rolls historical files
/// (`<stem>.1.<ext>`, `<stem>.2.<ext>`, ...) when the current file reaches `max_size_bytes`.
pub struct RotatingFileWriter {
    path: PathBuf,
    max_size_bytes: u64,
    max_files: usize,
    current_size: u64,
    file: Option<File>,
}

impl RotatingFileWriter {
    /// Opens or creates the target log file in append mode.
    ///
    /// If an existing legacy `.log.1` file exists from earlier versions, it will be
    /// migrated to `.1.log` automatically.
    /// If the existing log file is already at or above `max_size_bytes`, it is immediately rotated.
    pub fn new(path: PathBuf, max_size_bytes: u64, max_files: usize) -> io::Result<Self> {
        let max_size_bytes = max_size_bytes.max(1);
        let max_files = max_files.max(1);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Migrate legacy `velowork.log.1` -> `velowork.1.log` if found
        let legacy_prev = path.with_extension("log.1");
        let modern_1 = numbered_path(&path, 1);
        if legacy_prev.exists() && !modern_1.exists() {
            let _ = fs::rename(&legacy_prev, &modern_1);
        }

        let mut writer = Self {
            path,
            max_size_bytes,
            max_files,
            current_size: 0,
            file: None,
        };

        writer.open_file()?;

        // If file already exceeds limit at startup, rotate it immediately
        if writer.current_size >= writer.max_size_bytes {
            writer.rotate()?;
        }

        Ok(writer)
    }

    /// Internal helper to open the current log file with append mode.
    fn open_file(&mut self) -> io::Result<()> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let initial_size = file.metadata().map(|m| m.len()).unwrap_or(0);
        self.file = Some(file);
        self.current_size = initial_size;
        Ok(())
    }

    /// Perform the cascading rotation of backup log files.
    ///
    /// Note: we explicitly drop `self.file` before renaming to prevent Windows
    /// file sharing violations (`ERROR_SHARING_VIOLATION`).
    pub fn rotate(&mut self) -> io::Result<()> {
        // 1. Flush and release open file handle
        if let Some(mut f) = self.file.take() {
            let _ = f.flush();
        }

        // 2. Remove oldest backup if at limit
        let oldest = numbered_path(&self.path, self.max_files);
        if oldest.exists() {
            let _ = fs::remove_file(&oldest);
        }

        // 3. Shift intermediate backups: N-1 -> N, N-2 -> N-1, ...
        for i in (1..self.max_files).rev() {
            let src = numbered_path(&self.path, i);
            let dst = numbered_path(&self.path, i + 1);
            if src.exists() {
                let _ = fs::rename(&src, &dst);
            }
        }

        // 4. Move current file to .1.log
        if self.path.exists() {
            let dst = numbered_path(&self.path, 1);
            let _ = fs::rename(&self.path, &dst);
        }

        // 5. Re-open fresh log file
        self.open_file()?;
        self.current_size = 0;
        Ok(())
    }

    /// Current size of the active log file in bytes.
    pub fn current_size(&self) -> u64 {
        self.current_size
    }

    /// Target path of the active log file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Write for RotatingFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        // If writing this buffer would exceed limit and file has content, rotate first
        if self.current_size > 0
            && self.current_size + (buf.len() as u64) > self.max_size_bytes
            && let Err(err) = self.rotate()
        {
            eprintln!("velowork: log rotation error: {err}");
        }

        // If file is currently closed (e.g. failed earlier), try opening it
        if self.file.is_none() {
            self.open_file()?;
        }

        if let Some(file) = self.file.as_mut() {
            let written = file.write(buf)?;
            self.current_size += written as u64;
            Ok(written)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "Log file handle not available",
            ))
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = self.file.as_mut() {
            file.flush()
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_numbered_path_generation() {
        let p = Path::new("/var/log/velowork.log");
        assert_eq!(
            numbered_path(p, 1),
            PathBuf::from("/var/log/velowork.1.log")
        );
        assert_eq!(
            numbered_path(p, 5),
            PathBuf::from("/var/log/velowork.5.log")
        );

        let no_ext = Path::new("/var/log/velowork");
        assert_eq!(
            numbered_path(no_ext, 2),
            PathBuf::from("/var/log/velowork.2")
        );
    }

    #[test]
    fn test_rotating_file_writer_append_on_start() {
        let dir = tempdir().unwrap();
        let log_file = dir.path().join("velowork.log");

        // Pre-populate log file
        fs::write(&log_file, b"existing line\n").unwrap();

        // Open in append mode
        let mut writer = RotatingFileWriter::new(log_file.clone(), 1024, 3).unwrap();
        assert_eq!(writer.current_size(), 14);

        writer.write_all(b"second line\n").unwrap();
        writer.flush().unwrap();

        let content = fs::read_to_string(&log_file).unwrap();
        assert_eq!(content, "existing line\nsecond line\n");
    }

    #[test]
    fn test_rotating_file_writer_rotation_triggers_at_limit() {
        let dir = tempdir().unwrap();
        let log_file = dir.path().join("velowork.log");

        // Small limit: 50 bytes, max 3 files
        let mut writer = RotatingFileWriter::new(log_file.clone(), 50, 3).unwrap();

        // Write 30 bytes
        writer.write_all(&[b'a'; 30]).unwrap();
        assert_eq!(writer.current_size(), 30);
        assert!(!numbered_path(&log_file, 1).exists());

        // Write 25 bytes -> total 55 > 50 -> triggers rotation before write!
        writer.write_all(&[b'b'; 25]).unwrap();
        writer.flush().unwrap();

        let p1 = numbered_path(&log_file, 1);
        assert!(p1.exists(), "velowork.1.log should exist after rotation");
        assert_eq!(fs::read(&p1).unwrap(), vec![b'a'; 30]);

        // Active file now has the newly written 25 bytes
        assert_eq!(fs::read(&log_file).unwrap(), vec![b'b'; 25]);
        assert_eq!(writer.current_size(), 25);
    }

    #[test]
    fn test_rotating_file_writer_cascading_rotation_and_cleanup() {
        let dir = tempdir().unwrap();
        let log_file = dir.path().join("velowork.log");

        // Max 2 backups: velowork.log, velowork.1.log, velowork.2.log
        let mut writer = RotatingFileWriter::new(log_file.clone(), 20, 2).unwrap();

        // Round 1
        writer.write_all(b"batch 1 (len: 16)").unwrap(); // 16 bytes
        // Round 2: triggers rotation 1
        writer.write_all(b"batch 2 (len: 16)").unwrap(); // rotates, 16 bytes
        let p1 = numbered_path(&log_file, 1);
        let p2 = numbered_path(&log_file, 2);
        let p3 = numbered_path(&log_file, 3);
        assert!(p1.exists());
        assert_eq!(fs::read(&p1).unwrap(), b"batch 1 (len: 16)");

        // Round 3: triggers rotation 2
        writer.write_all(b"batch 3 (len: 16)").unwrap();
        assert!(p1.exists());
        assert!(p2.exists());
        assert_eq!(fs::read(&p1).unwrap(), b"batch 2 (len: 16)");
        assert_eq!(fs::read(&p2).unwrap(), b"batch 1 (len: 16)");

        // Round 4: triggers rotation 3 -> batch 1 should be discarded since max_files=2
        writer.write_all(b"batch 4 (len: 16)").unwrap();
        assert!(p1.exists());
        assert!(p2.exists());
        assert!(!p3.exists(), "velowork.3.log should not exist (max_files=2)");
        assert_eq!(fs::read(&p1).unwrap(), b"batch 3 (len: 16)");
        assert_eq!(fs::read(&p2).unwrap(), b"batch 2 (len: 16)");
        assert_eq!(fs::read(&log_file).unwrap(), b"batch 4 (len: 16)");
    }

    #[test]
    fn test_legacy_log_1_migration() {
        let dir = tempdir().unwrap();
        let log_file = dir.path().join("velowork.log");
        let legacy = dir.path().join("velowork.log.1");

        fs::write(&legacy, b"old legacy log content").unwrap();

        let _writer = RotatingFileWriter::new(log_file.clone(), 1024, 3).unwrap();
        assert!(!legacy.exists(), "legacy file should have been moved");
        let modern_1 = numbered_path(&log_file, 1);
        assert!(modern_1.exists(), "velowork.1.log should exist now");
        assert_eq!(fs::read_to_string(&modern_1).unwrap(), "old legacy log content");
    }

    #[test]
    fn test_env_parsing_fallbacks() {
        // Just verify standard fallback values work
        assert_eq!(DEFAULT_MAX_LOG_SIZE_BYTES, 10 * 1024 * 1024);
        assert_eq!(DEFAULT_MAX_LOG_FILES, 5);
    }
}
