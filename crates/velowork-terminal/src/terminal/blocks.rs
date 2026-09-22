//! Terminal Blocks: structured command execution records.
//!
//! Tracks command execution lifecycle via OSC 133 prompt marks:
//! - `PromptStart` / `CommandStart`: Shell is displaying prompt or ready for user input
//! - `CommandExecuted`: Command is submitted and starts executing
//! - `CommandFinished`: Command finished with exit code and duration
//!
//! Produces structured [`TerminalBlock`] records containing command text,
//! cleaned output, exit code, execution duration, and cwd snapshot.

use std::collections::VecDeque;
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static RE_ANSI: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\x1b\[[0-9;?]*[a-zA-Z]|\x1b\].*?(\x07|\x1b\\)|\r").expect("valid regex")
});

/// Strip ANSI escape codes and normalize carriage returns.
pub fn strip_ansi(input: &str) -> String {
    RE_ANSI.replace_all(input, "").to_string()
}

/// Head-tail truncation to avoid memory and token explosion.
pub fn truncate_output(input: &str, max_chars: usize, head_chars: usize, tail_chars: usize) -> (String, bool) {
    if input.chars().count() <= max_chars {
        return (input.to_string(), false);
    }

    let total = input.chars().count();
    let head: String = input.chars().take(head_chars).collect();
    let skip = total.saturating_sub(tail_chars);
    let tail: String = input.chars().skip(skip).collect();
    let omitted = total.saturating_sub(head_chars + tail_chars);

    (
        format!("{}\n\n[... omitted {} characters ...]\n\n{}", head, omitted, tail),
        true,
    )
}

/// A structured record of a single command executed in the terminal.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct TerminalBlock {
    pub id: u64,
    pub command: String,
    pub raw_output: String,
    pub clean_output: String,
    pub exit_code: Option<i32>,
    pub started_at_ms: u64,
    pub duration_ms: Option<u64>,
    pub cwd: Option<String>,
    pub is_truncated: bool,
    pub is_collapsed: bool,
}

impl TerminalBlock {
    /// Whether the command succeeded (exit code is 0).
    pub fn is_success(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// Internal state while a command is currently running.
#[derive(Clone, Debug)]
struct PendingCommand {
    start_line: i32,
    started_at: Instant,
    started_at_ms: u64,
    command: String,
    cwd: Option<String>,
}

/// Ring buffer tracker for terminal execution blocks.
pub struct BlockTracker {
    blocks: VecDeque<TerminalBlock>,
    pending: Option<PendingCommand>,
    capacity: usize,
    next_id: u64,
}

impl Default for BlockTracker {
    fn default() -> Self {
        Self::new(100)
    }
}

impl BlockTracker {
    pub fn new(capacity: usize) -> Self {
        Self {
            blocks: VecDeque::with_capacity(capacity),
            pending: None,
            capacity: capacity.max(16),
            next_id: 1,
        }
    }

    /// Record that a command started executing at the given buffer line.
    pub fn on_command_executed(
        &mut self,
        line: i32,
        command: Option<String>,
        cwd: Option<String>,
    ) {
        let now = Instant::now();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        self.pending = Some(PendingCommand {
            start_line: line,
            started_at: now,
            started_at_ms: now_ms,
            command: command.unwrap_or_default(),
            cwd,
        });
    }

    /// Record that a command finished. Extracts output text using the provided closure.
    pub fn on_command_finished<F>(
        &mut self,
        end_line: i32,
        exit_code: Option<i32>,
        output_extractor: F,
    ) -> Option<TerminalBlock>
    where
        F: FnOnce(i32, i32) -> String,
    {
        let pending = self.pending.take()?;
        let duration = pending.started_at.elapsed();
        let duration_ms = Some(duration.as_millis() as u64);

        let raw_output = output_extractor(pending.start_line, end_line);
        let cleaned = strip_ansi(&raw_output);
        let (clean_output, is_truncated) = truncate_output(&cleaned, 65536, 1500, 2500);

        let block = TerminalBlock {
            id: self.next_id,
            command: pending.command,
            raw_output,
            clean_output,
            exit_code,
            started_at_ms: pending.started_at_ms,
            duration_ms,
            cwd: pending.cwd,
            is_truncated,
            is_collapsed: false,
        };
        self.next_id += 1;

        if self.blocks.len() >= self.capacity {
            self.blocks.pop_front();
        }
        self.blocks.push_back(block.clone());
        Some(block)
    }

    /// Directly record a synthetic block (e.g. from sentinel output parsing).
    pub fn record_synthetic_block(
        &mut self,
        command: String,
        output: String,
        exit_code: Option<i32>,
        duration: Duration,
        cwd: Option<String>,
    ) -> TerminalBlock {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let cleaned = strip_ansi(&output);
        let (clean_output, is_truncated) = truncate_output(&cleaned, 65536, 1500, 2500);

        let block = TerminalBlock {
            id: self.next_id,
            command,
            raw_output: output,
            clean_output,
            exit_code,
            started_at_ms: now_ms.saturating_sub(duration.as_millis() as u64),
            duration_ms: Some(duration.as_millis() as u64),
            cwd,
            is_truncated,
            is_collapsed: false,
        };
        self.next_id += 1;

        if self.blocks.len() >= self.capacity {
            self.blocks.pop_front();
        }
        self.blocks.push_back(block.clone());
        block
    }

    /// Shift the pending command's `start_line` upward when history lines scroll into scrollback.
    pub fn on_history_changed(&mut self, delta: usize, topmost: i32) {
        if delta == 0 {
            return;
        }
        let delta_i32 = delta as i32;
        if let Some(ref mut pending) = self.pending {
            pending.start_line -= delta_i32;
            if pending.start_line < topmost {
                pending.start_line = topmost;
            }
        }
    }

    /// Return all tracked blocks, oldest first.
    pub fn blocks(&self) -> Vec<TerminalBlock> {
        self.blocks.iter().cloned().collect()
    }

    /// Return the most recent completed block.
    pub fn last_block(&self) -> Option<TerminalBlock> {
        self.blocks.back().cloned()
    }

    /// Toggle collapsed status of a block.
    pub fn toggle_collapsed(&mut self, id: u64) -> bool {
        if let Some(block) = self.blocks.iter_mut().find(|b| b.id == id) {
            block.is_collapsed = !block.is_collapsed;
            true
        } else {
            false
        }
    }

    /// Whether a command is currently executing.
    pub fn is_executing(&self) -> bool {
        self.pending.is_some()
    }
}

/// Errors that can occur during command execution capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandCaptureError {
    Busy,
    Timeout(Duration),
    AlternateScreen,
    ExecutionFailed(String),
}

impl std::fmt::Display for CommandCaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => write!(f, "terminal is currently busy with another command execution"),
            Self::Timeout(dur) => write!(f, "command execution timed out after {:?}", dur),
            Self::AlternateScreen => write!(f, "terminal entered alternate screen / interactive TUI mode"),
            Self::ExecutionFailed(msg) => write!(f, "execution failed: {}", msg),
        }
    }
}

impl std::error::Error for CommandCaptureError {}

impl super::Terminal {
    /// Execute a shell command and capture its output and exit code.
    ///
    /// Uses a hybrid strategy:
    /// 1. Subscribes to OSC 133 command-finished broadcasts.
    /// 2. Wraps the command in unique sentinel markers (`__VELO_CMD_START__` / `__VELO_CMD_END__`)
    ///    so that commands on remote SSH sessions or shells without OSC 133 still capture output accurately.
    /// 3. Detects alternate screen buffer (vim, less) and aborts early.
    /// 4. Respects `timeout` duration and acquires terminal capture lock to prevent concurrent pollution.
    pub async fn execute_command_and_capture(
        &self,
        cmd: &str,
        timeout: Duration,
    ) -> Result<TerminalBlock, CommandCaptureError> {
        let _guard = self.acquire_capture_lock().ok_or(CommandCaptureError::Busy)?;

        if self.is_alt_screen() {
            return Err(CommandCaptureError::AlternateScreen);
        }

        let token = uuid::Uuid::new_v4().simple().to_string();
        let wrapped_cmd = crate::shell_integration::wrap_command_with_sentinel(cmd, &token);

        let mut osc_rx = self.subscribe_command_finished();
        let start_time = Instant::now();

        // Send wrapped command with carriage return
        let mut send_bytes = wrapped_cmd.into_bytes();
        if !send_bytes.ends_with(b"\r") && !send_bytes.ends_with(b"\n") {
            send_bytes.push(b'\r');
        }
        self.send_bytes(&send_bytes);

        let deadline = start_time + timeout;

        loop {
            if Instant::now() > deadline {
                return Err(CommandCaptureError::Timeout(timeout));
            }

            if self.is_alt_screen() {
                return Err(CommandCaptureError::AlternateScreen);
            }

            // Check if OSC 133 captured a block
            while let Ok(block) = osc_rx.try_recv() {
                if block.command.contains(cmd) || block.raw_output.contains(&token) {
                    if let Some(parsed) = crate::shell_integration::parse_sentinel_output(&block.raw_output, &token) {
                        let synthetic = self.block_tracker.lock().record_synthetic_block(
                            cmd.to_string(),
                            parsed.output,
                            parsed.exit_code,
                            start_time.elapsed(),
                            self.current_cwd_opt(),
                        );
                        return Ok(synthetic);
                    } else {
                        return Ok((*block).clone());
                    }
                }
            }

            // Also check terminal buffer directly for sentinel markers (for non-OSC 133 shells)
            let buffer_text = self.get_recent_lines(150);
            if let Some(parsed) = crate::shell_integration::parse_sentinel_output(&buffer_text, &token) {
                let synthetic = self.block_tracker.lock().record_synthetic_block(
                    cmd.to_string(),
                    parsed.output,
                    parsed.exit_code,
                    start_time.elapsed(),
                    self.current_cwd_opt(),
                );
                return Ok(synthetic);
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn current_cwd_opt(&self) -> Option<String> {
        let cwd = self.current_cwd();
        if cwd.is_empty() { None } else { Some(cwd) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_lifecycle() {
        let mut tracker = BlockTracker::new(10);
        assert!(!tracker.is_executing());

        tracker.on_command_executed(0, Some("echo 'hello'".into()), Some("/home".into()));
        assert!(tracker.is_executing());

        let block = tracker
            .on_command_finished(1, Some(0), |_start, _end| "hello\r\n".to_string())
            .expect("should produce block");

        assert_eq!(block.id, 1);
        assert_eq!(block.command, "echo 'hello'");
        assert_eq!(block.clean_output, "hello\n");
        assert_eq!(block.exit_code, Some(0));
        assert!(block.is_success());
        assert!(!tracker.is_executing());

        assert_eq!(tracker.blocks().len(), 1);
        assert_eq!(tracker.last_block().map(|b| b.id), Some(1));
    }

    #[test]
    fn test_truncate_output() {
        let long_str = "a".repeat(1000);
        let (truncated, is_truncated) = truncate_output(&long_str, 100, 20, 20);
        assert!(is_truncated);
        assert!(truncated.contains("[... omitted 960 characters ...]"));
    }
}
