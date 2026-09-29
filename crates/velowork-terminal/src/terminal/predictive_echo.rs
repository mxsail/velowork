//! Predictive Local Echo Tracker.
//!
//! Provides instantaneous (0ms) speculative local visual feedback for remote
//! terminals (SSH/Telnet) under high network latency or packet loss.

use alacritty_terminal::index::{Column, Point};
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthChar;

pub const MAX_PREDICTABLE_INPUT_LEN: usize = 128;
pub const PREDICTION_TIMEOUT: Duration = Duration::from_millis(3000);

/// A single predicted speculative character waiting for remote echo confirmation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PredictedChar {
    pub ch: char,
    pub width: u8,
    pub point: Point,
    pub timestamp: Instant,
}

/// State tracker for speculative local character prediction and reconciliation.
#[derive(Debug, Default)]
pub struct PredictiveEchoTracker {
    is_remote: bool,
    predictions: Vec<PredictedChar>,
}

/// Determines if a trimmed terminal text line represents an interactive password / authentication prompt.
pub fn is_auth_prompt_line(line: &str) -> bool {
    let lower = line.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }

    let has_shell_marker = lower.contains("$ ")
        || lower.contains("# ")
        || lower.contains("% ")
        || lower.contains("❯ ")
        || lower.contains("➜ ")
        || lower.contains("» ");

    let has_password_keyword = lower.contains("password")
        || lower.contains("passphrase")
        || lower.contains("passcode")
        || lower.contains("密码")
        || lower.contains("口令");

    let has_2fa_keyword = lower.contains("verification code")
        || lower.contains("authenticator")
        || lower.contains("otp")
        || lower.contains("totp")
        || lower.contains("2fa")
        || lower.contains("mfa")
        || lower.contains("security code")
        || lower.contains("动态码")
        || lower.contains("动态口令")
        || lower.contains("验证码")
        || lower.contains("一次性口令")
        || lower.contains("两步验证")
        || lower.starts_with("pin:")
        || lower.contains("enter pin")
        || lower.contains("pin for");

    if !has_password_keyword && !has_2fa_keyword {
        return false;
    }

    if has_shell_marker {
        if let Some(marker_pos) = lower
            .find("$ ")
            .or_else(|| lower.find("# "))
            .or_else(|| lower.find("% "))
            .or_else(|| lower.find("❯ "))
            .or_else(|| lower.find("➜ "))
        {
            let prefix = &lower[..marker_pos];
            if !prefix.contains("password")
                && !prefix.contains("密码")
                && !prefix.contains("口令")
                && !prefix.contains("passphrase")
            {
                return false;
            }
        }
    }

    true
}

impl PredictiveEchoTracker {
    pub fn new() -> Self {
        Self {
            is_remote: false,
            predictions: Vec::new(),
        }
    }

    /// Mark whether this terminal represents a remote session (SSH / Telnet).
    pub fn set_remote(&mut self, is_remote: bool) {
        if self.is_remote != is_remote {
            self.is_remote = is_remote;
            self.predictions.clear();
        }
    }

    /// Check if predictive echo is enabled for this terminal.
    pub fn is_remote(&self) -> bool {
        self.is_remote
    }

    /// Check if there are active unconfirmed predictions.
    pub fn has_predictions(&self) -> bool {
        !self.predictions.is_empty()
    }

    /// Get current active non-expired predictions.
    pub fn predictions(&self) -> Vec<PredictedChar> {
        let now = Instant::now();
        self.predictions
            .iter()
            .filter(|p| now.duration_since(p.timestamp) < PREDICTION_TIMEOUT)
            .cloned()
            .collect()
    }

    /// Clear all pending predictions (e.g. on Enter, Ctrl+C, Resize, AltScreen).
    pub fn clear(&mut self) {
        self.predictions.clear();
    }

    /// Compute the current predicted cursor position (the cell immediately following
    /// the last predicted character), or `None` if no predictions are pending.
    pub fn predicted_cursor(&self, max_cols: usize) -> Option<Point> {
        let now = Instant::now();
        let last = self
            .predictions
            .iter()
            .rev()
            .find(|p| now.duration_since(p.timestamp) < PREDICTION_TIMEOUT)?;
        let next_col = (last.point.column.0 + last.width as usize).min(max_cols.saturating_sub(1));
        Some(Point {
            line: last.point.line,
            column: Column(next_col),
        })
    }

    /// Attempt to predict user input characters starting from `real_cursor`.
    ///
    /// Returns `true` if one or more characters were successfully predicted.
    pub fn predict_input(&mut self, text: &str, real_cursor: Point, max_cols: usize) -> bool {
        if !self.is_remote || max_cols == 0 {
            return false;
        }

        // Multi-line paste or oversized input: do not predict
        if text.contains('\n') || text.contains('\r') || text.len() > MAX_PREDICTABLE_INPUT_LEN {
            self.clear();
            return false;
        }

        let start_point = self
            .predicted_cursor(max_cols)
            .unwrap_or(real_cursor);

        let mut current_col = start_point.column.0;
        let line = start_point.line;
        let mut predicted_any = false;

        for c in text.chars() {
            if c.is_control() {
                self.clear();
                return false;
            }

            let width = c.width().unwrap_or(1) as u8;
            if width == 0 {
                continue;
            }

            // Margin check: if character would overflow beyond screen width, stop predicting
            if current_col + width as usize > max_cols {
                break;
            }

            self.predictions.push(PredictedChar {
                ch: c,
                width,
                point: Point {
                    line,
                    column: Column(current_col),
                },
                timestamp: Instant::now(),
            });

            current_col += width as usize;
            predicted_any = true;
        }

        predicted_any
    }

    /// Pop the last prediction when user presses Backspace.
    ///
    /// Returns `true` if a prediction was removed.
    pub fn predict_backspace(&mut self) -> bool {
        if !self.is_remote {
            return false;
        }
        self.predictions.pop().is_some()
    }

    /// Reconcile predictions against real server cursor position after output arrives.
    pub fn on_remote_output(&mut self, real_cursor: Point) {
        if self.predictions.is_empty() {
            return;
        }

        // 1. Drop expired predictions
        self.predictions.retain(|p| p.timestamp.elapsed() < PREDICTION_TIMEOUT);

        if self.predictions.is_empty() {
            return;
        }

        // 2. If server moved to a different line or cursor moved backwards before first prediction, clear
        let pred_first = &self.predictions[0];
        if real_cursor.line != pred_first.point.line || real_cursor.column.0 < pred_first.point.column.0 {
            self.clear();
            return;
        }

        // 3. Drop predictions that have been reached or passed by real cursor
        self.predictions.retain(|p| p.point.column.0 >= real_cursor.column.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_predict_and_reconcile() {
        let mut tracker = PredictiveEchoTracker::new();
        assert!(!tracker.predict_input("ls", Point { line: alacritty_terminal::index::Line(0), column: Column(0) }, 80));

        tracker.set_remote(true);
        let real_cursor = Point { line: alacritty_terminal::index::Line(0), column: Column(0) };
        assert!(tracker.predict_input("ls", real_cursor, 80));
        assert_eq!(tracker.predictions().len(), 2);
        assert_eq!(tracker.predictions()[0].ch, 'l');
        assert_eq!(tracker.predictions()[0].point.column.0, 0);
        assert_eq!(tracker.predictions()[1].ch, 's');
        assert_eq!(tracker.predictions()[1].point.column.0, 1);
        assert_eq!(tracker.predicted_cursor(80), Some(Point { line: alacritty_terminal::index::Line(0), column: Column(2) }));

        // Remote confirms 'l' (real cursor moves to column 1)
        tracker.on_remote_output(Point { line: alacritty_terminal::index::Line(0), column: Column(1) });
        assert_eq!(tracker.predictions().len(), 1);
        assert_eq!(tracker.predictions()[0].ch, 's');

        // Remote confirms 's' (real cursor moves to column 2)
        tracker.on_remote_output(Point { line: alacritty_terminal::index::Line(0), column: Column(2) });
        assert!(tracker.predictions().is_empty());
    }

    #[test]
    fn test_cjk_double_width() {
        let mut tracker = PredictiveEchoTracker::new();
        tracker.set_remote(true);
        let real_cursor = Point { line: alacritty_terminal::index::Line(0), column: Column(0) };
        assert!(tracker.predict_input("中", real_cursor, 80));
        assert_eq!(tracker.predictions().len(), 1);
        assert_eq!(tracker.predictions()[0].width, 2);
        assert_eq!(tracker.predicted_cursor(80), Some(Point { line: alacritty_terminal::index::Line(0), column: Column(2) }));
    }

    #[test]
    fn test_backspace() {
        let mut tracker = PredictiveEchoTracker::new();
        tracker.set_remote(true);
        let real_cursor = Point { line: alacritty_terminal::index::Line(0), column: Column(0) };
        tracker.predict_input("ab", real_cursor, 80);
        assert_eq!(tracker.predictions().len(), 2);

        assert!(tracker.predict_backspace());
        assert_eq!(tracker.predictions().len(), 1);
        assert_eq!(tracker.predictions()[0].ch, 'a');

        assert!(tracker.predict_backspace());
        assert!(tracker.predictions().is_empty());

        assert!(!tracker.predict_backspace());
    }

    #[test]
    fn test_margin_overflow_stops_prediction() {
        let mut tracker = PredictiveEchoTracker::new();
        tracker.set_remote(true);
        let real_cursor = Point { line: alacritty_terminal::index::Line(0), column: Column(78) };
        // Trying to type 4 characters with max_cols = 80
        tracker.predict_input("abcd", real_cursor, 80);
        // Only 2 fit ('a' at 78, 'b' at 79)
        assert_eq!(tracker.predictions().len(), 2);
    }
}
