use alacritty_terminal::event::{Event as TermEvent, EventListener};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use velowork_core::types::BellStyle;

use super::transport::TerminalTransport;

/// 终端 Bell 运行时状态与节流（每个 Terminal 实例独立）。
#[derive(Clone, Debug)]
pub struct BellRuntimeState {
    pub style: BellStyle,
    pub cooldown_ms: u32,
    pub last_bell_time: Option<Instant>,
}

impl Default for BellRuntimeState {
    fn default() -> Self {
        Self {
            style: BellStyle::Visual,
            cooldown_ms: 500,
            last_bell_time: None,
        }
    }
}

/// 触发跨平台原生系统蜂鸣/提示音，安全降级。
pub fn play_system_beep() {
    #[cfg(target_os = "windows")]
    unsafe {
        #[link(name = "user32")]
        extern "system" {
            fn MessageBeep(uType: u32) -> i32;
        }
        MessageBeep(0);
    }
    #[cfg(target_os = "macos")]
    {
        std::thread::spawn(|| {
            let _ = std::process::Command::new("afplay")
                .arg("/System/Library/Sounds/Ping.aiff")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        });
    }
    #[cfg(target_os = "linux")]
    {
        std::thread::spawn(|| {
            let sound_files = [
                "/usr/share/sounds/freedesktop/stereo/bell.oga",
                "/usr/share/sounds/ocean/stereo/bell.oga",
                "/usr/share/sounds/ocean/stereo/bell-window-system.oga",
                "/usr/share/sounds/freedesktop/stereo/dialog-warning.oga",
                "/usr/share/sounds/gnome/default/alerts/glass.ogg",
                "/usr/share/sounds/ubuntu/stereo/bell.ogg",
            ];
            let sound_file = sound_files.iter().find(|p| std::path::Path::new(p).exists());

            let mut played = false;
            if let Some(path) = sound_file {
                for player in &["paplay", "pw-play", "aplay"] {
                    if let Ok(mut child) = std::process::Command::new(player)
                        .arg(path)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                    {
                        let _ = child.wait();
                        played = true;
                        break;
                    }
                }
            }

            if !played {
                let _ = std::process::Command::new("canberra-gtk-play")
                    .arg("-i")
                    .arg("bell")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
        });
    }
}

/// Event listener for alacritty_terminal that captures title changes, bell, and PTY write requests
pub struct ZedEventListener {
    /// Shared title storage - OSC 0/1/2 sequences update this
    title: Arc<Mutex<Option<String>>>,
    /// Sticky bell flag for the UI (red border / sidebar dot), cleared on focus.
    has_bell: Arc<Mutex<bool>>,
    /// One-shot "the bell rang since the last drain" edge, consumed by the PTY
    /// event loop to fire a desktop notification exactly once per bell rather
    /// than on every batch while `has_bell` stays set.
    bell_pending: Arc<AtomicBool>,
    /// Terminal Bell 运行时配置与冷却状态
    bell_state: Arc<Mutex<BellRuntimeState>>,
    /// Pending OSC 52 clipboard writes to be picked up by the GPUI thread
    pending_clipboard: Arc<Mutex<Vec<String>>>,
    /// Current terminal palette, pushed from the GPUI thread on each render.
    /// Used to answer OSC 10/11/12/4 color queries from apps.
    palette: Arc<Mutex<Option<velowork_core::theme::TerminalPalette>>>,
    /// Transport for writing responses back to the terminal
    transport: Arc<dyn TerminalTransport>,
    /// Terminal ID for PTY write operations
    terminal_id: String,
}

impl ZedEventListener {
    pub fn new(
        title: Arc<Mutex<Option<String>>>,
        has_bell: Arc<Mutex<bool>>,
        bell_pending: Arc<AtomicBool>,
        bell_state: Arc<Mutex<BellRuntimeState>>,
        pending_clipboard: Arc<Mutex<Vec<String>>>,
        palette: Arc<Mutex<Option<velowork_core::theme::TerminalPalette>>>,
        transport: Arc<dyn TerminalTransport>,
        terminal_id: String,
    ) -> Self {
        Self {
            title,
            has_bell,
            bell_pending,
            bell_state,
            pending_clipboard,
            palette,
            transport,
            terminal_id,
        }
    }

    /// Resolve a color index (as passed by alacritty on a color query) to an
    /// (r, g, b) triple.
    ///
    /// Indices 0..=15 and the named foreground/background/cursor slots come
    /// from the active terminal palette (so apps that ask "what's your red?"
    /// see Velowork's configured red, not xterm's). Indices 16..=231 and the
    /// 24-step grayscale ramp 232..=255 are answered from the standard
    /// xterm 256-color table — these are not themed and match every other
    /// modern terminal.
    fn resolve_color(&self, index: usize) -> Option<(u8, u8, u8)> {
        use alacritty_terminal::vte::ansi::NamedColor;

        if (16..=231).contains(&index) {
            return Some(xterm_256_cube_rgb(index));
        }
        if (232..=255).contains(&index) {
            return Some(xterm_256_grayscale_rgb(index));
        }

        let palette_guard = self.palette.lock();
        let p = palette_guard.as_ref()?;
        let hex = match index {
            0 => p.black,
            1 => p.red,
            2 => p.green,
            3 => p.yellow,
            4 => p.blue,
            5 => p.magenta,
            6 => p.cyan,
            7 => p.white,
            8 => p.bright_black,
            9 => p.bright_red,
            10 => p.bright_green,
            11 => p.bright_yellow,
            12 => p.bright_blue,
            13 => p.bright_magenta,
            14 => p.bright_cyan,
            15 => p.bright_white,
            i if i == NamedColor::Foreground as usize => p.foreground,
            i if i == NamedColor::Background as usize => p.background,
            i if i == NamedColor::Cursor as usize => p.foreground,
            _ => return None,
        };
        Some(((hex >> 16) as u8, (hex >> 8) as u8, hex as u8))
    }
}

/// xterm 6x6x6 color cube for palette indices 16..=231.
///
/// Each axis uses the canonical xterm levels [0, 95, 135, 175, 215, 255]
/// (not a linear 0/51/102/... ramp — xterm jumps at 95 for perceptual
/// reasons and every modern terminal matches this).
pub(super) fn xterm_256_cube_rgb(index: usize) -> (u8, u8, u8) {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let n = index - 16;
    (LEVELS[n / 36], LEVELS[(n / 6) % 6], LEVELS[n % 6])
}

/// xterm 24-step grayscale ramp for palette indices 232..=255. The levels
/// start at 8 and step by 10 (8, 18, ..., 238), skipping true black and
/// true white — apps that need those use cube indices 16 and 231.
pub(super) fn xterm_256_grayscale_rgb(index: usize) -> (u8, u8, u8) {
    let level = 8 + (index as u8 - 232) * 10;
    (level, level, level)
}

impl EventListener for ZedEventListener {
    fn send_event(&self, event: TermEvent) {
        match event {
            TermEvent::Title(title) => {
                *self.title.lock() = Some(title);
            }
            TermEvent::ResetTitle => {
                *self.title.lock() = None;
            }
            TermEvent::Bell => {
                let mut state = self.bell_state.lock();
                let now = Instant::now();
                if let Some(prev) = state.last_bell_time {
                    if state.cooldown_ms > 0
                        && now.duration_since(prev).as_millis() < state.cooldown_ms as u128
                    {
                        return; // 被冷却节流拦截，不执行也不刷新 last_bell_time
                    }
                }
                state.last_bell_time = Some(now); // 仅在真正放行时更新
                let style = state.style;
                drop(state);

                if matches!(style, BellStyle::Visual | BellStyle::Both) {
                    *self.has_bell.lock() = true;
                    self.bell_pending.store(true, Ordering::Relaxed);
                }
                if matches!(style, BellStyle::Audible | BellStyle::Both) {
                    play_system_beep();
                }
            }
            TermEvent::ClipboardStore(_, text) => {
                self.pending_clipboard.lock().push(text);
            }
            TermEvent::ColorRequest(index, response_fn) => {
                if let Some((r, g, b)) = self.resolve_color(index) {
                    let reply =
                        response_fn(alacritty_terminal::vte::ansi::Rgb { r, g, b });
                    self.transport.send_input(&self.terminal_id, reply.as_bytes());
                }
            }
            TermEvent::PtyWrite(data) => {
                // Write response back to PTY (e.g., cursor position report)
                log::debug!("PtyWrite event: {:?}", data);
                self.transport.send_input(&self.terminal_id, data.as_bytes());
            }
            _ => {
                // Ignore other events
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyTransport;
    impl TerminalTransport for DummyTransport {
        fn send_input(&self, _terminal_id: &str, _data: &[u8]) {}
        fn resize(&self, _terminal_id: &str, _cols: u16, _rows: u16) {}
        fn uses_mouse_backend(&self) -> bool {
            false
        }
    }

    #[test]
    fn test_bell_event_cooldown_and_styles() {
        let title = Arc::new(Mutex::new(None));
        let has_bell = Arc::new(Mutex::new(false));
        let bell_pending = Arc::new(AtomicBool::new(false));
        let bell_state = Arc::new(Mutex::new(BellRuntimeState {
            style: BellStyle::Visual,
            cooldown_ms: 200,
            last_bell_time: None,
        }));
        let pending_clipboard = Arc::new(Mutex::new(Vec::new()));
        let palette = Arc::new(Mutex::new(None));
        let transport = Arc::new(DummyTransport);
        let terminal_id = "test-term".to_string();

        let listener = ZedEventListener::new(
            title,
            has_bell.clone(),
            bell_pending.clone(),
            bell_state.clone(),
            pending_clipboard,
            palette,
            transport,
            terminal_id,
        );

        // 1. First bell (Visual, 200ms) -> accepted
        listener.send_event(TermEvent::Bell);
        assert!(*has_bell.lock());
        assert!(bell_pending.load(Ordering::Relaxed));

        // Clear flag
        *has_bell.lock() = false;
        bell_pending.store(false, Ordering::Relaxed);

        // 2. Second bell immediately -> throttled (no flag set)
        listener.send_event(TermEvent::Bell);
        assert!(!*has_bell.lock());
        assert!(!bell_pending.load(Ordering::Relaxed));

        // 3. Test Disabled style
        bell_state.lock().style = BellStyle::Disabled;
        bell_state.lock().last_bell_time = None; // Reset cooldown
        listener.send_event(TermEvent::Bell);
        assert!(!*has_bell.lock());
        assert!(!bell_pending.load(Ordering::Relaxed));

        // 4. Test cooldown = 0 (no throttle)
        bell_state.lock().style = BellStyle::Visual;
        bell_state.lock().cooldown_ms = 0;
        bell_state.lock().last_bell_time = None;

        listener.send_event(TermEvent::Bell);
        assert!(*has_bell.lock());
        *has_bell.lock() = false;

        listener.send_event(TermEvent::Bell);
        assert!(*has_bell.lock());
    }
}
