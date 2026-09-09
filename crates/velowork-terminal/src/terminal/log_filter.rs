//! Stateful ANSI & VT100 control sequence filter for clean log recording.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    Csi,
    Osc,
    OscEsc,
    Charset,
}

/// Filter raw PTY bytes to extract clean, readable plain text.
pub struct PtyLogFilter {
    state: State,
    pending_cr: bool,
}

impl Default for PtyLogFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl PtyLogFilter {
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            pending_cr: false,
        }
    }

    pub fn filter(&mut self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        for &b in data {
            match self.state {
                State::Ground => {
                    if b == 0x1b {
                        if self.pending_cr {
                            self.pending_cr = false;
                        }
                        self.state = State::Escape;
                    } else if b == 0x0d {
                        self.pending_cr = true;
                    } else if b == 0x0a {
                        if self.pending_cr {
                            out.push(b'\r');
                            self.pending_cr = false;
                        }
                        out.push(b'\n');
                    } else if b == 0x08 || b == 0x07 {
                        // Skip backspace and BEL control characters
                    } else if b >= 0x20 || b == 0x09 {
                        if self.pending_cr {
                            self.pending_cr = false;
                        }
                        out.push(b);
                    }
                }
                State::Escape => {
                    match b {
                        b'[' => self.state = State::Csi,
                        b']' => self.state = State::Osc,
                        b'(' | b')' | b'*' | b'+' => self.state = State::Charset,
                        _ => self.state = State::Ground,
                    }
                }
                State::Csi => {
                    // Final byte of CSI sequence is in 0x40..=0x7e ('m', 'h', 'l', 'A', 'J', 'K', etc.)
                    if (0x40..=0x7e).contains(&b) {
                        self.state = State::Ground;
                    }
                }
                State::Osc => {
                    if b == 0x07 {
                        self.state = State::Ground;
                    } else if b == 0x1b {
                        self.state = State::OscEsc;
                    }
                }
                State::OscEsc => {
                    if b == b'\\' {
                        self.state = State::Ground;
                    } else {
                        self.state = State::Osc;
                    }
                }
                State::Charset => {
                    self.state = State::Ground;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_log_filter_strips_ansi() {
        let mut filter = PtyLogFilter::new();
        let raw = b"\x1b[32mhello\x1b[0m world\r\n\x1b[A\x1b[Jfoo";
        let cleaned = filter.filter(raw);
        assert_eq!(String::from_utf8_lossy(&cleaned), "hello world\r\nfoo");
    }
}
