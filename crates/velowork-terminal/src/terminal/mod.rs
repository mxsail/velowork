use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::vte::ansi::{CursorShape as VteCursorShape, CursorStyle as VteCursorStyle, Processor};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use std::time::Instant;

mod ansi_snapshot;
mod app_version;
mod child_processes;
mod event_listener;
mod idle;
mod io;
pub mod log_filter;
mod links;
mod meta;
mod mouse;
mod modes;
mod osc_sidecar;
mod preview;
mod prompt_jump;
mod prompt_marks;
mod render;
mod resize;
mod resize_authority;
mod scroll;
mod search;
mod selection;
mod transport;
mod types;
mod url_detect;

#[cfg(test)]
mod tests;

pub use app_version::set_app_version;
pub use child_processes::{foreground_command, has_child_processes};
pub use preview::{TerminalPreviewLine, TerminalPreviewSnapshot, TerminalPreviewSpan};
pub use resize_authority::{
    claim_resize_authority_local, claim_resize_authority_remote, is_resize_authority_local,
};
pub use transport::TerminalTransport;
pub use types::{
    AppCursorShape, DetectedLink, PromptMark, PromptMarkKind, ResizeState, SelectionState,
    TerminalSize,
};

pub use osc_sidecar::TerminalNotification;

use event_listener::ZedEventListener;
use osc_sidecar::OscSidecar;
use prompt_marks::{PromptSidecar, PromptTracker};
use types::FocusReportState;

/// A terminal instance wrapping alacritty_terminal
/// Terminal emulator state.
///
/// # Threading model
///
/// `Terminal` is always stored behind `Arc` (in `TerminalsRegistry`) and all
/// methods take `&self`, using interior mutability for mutation. Three
/// execution contexts access the struct:
///
/// 1. **GPUI thread** — the main UI thread. Runs `process_output` (via the
///    batched PTY event loop in `Velowork`), all rendering (`with_content`),
///    user-input methods, resize, selection, scroll, and idle-detection reads.
///    This is where the vast majority of field access happens.
///
/// 2. **Tokio reader task** (remote connections only) — calls `enqueue_output`
///    to buffer incoming data without holding `term.lock()`. Only touches
///    `pending_output`, `dirty`, and `last_output_time`.
///
/// 3. **Resize debounce timer** — a short-lived `std::thread::spawn` that
///    flushes a trailing-edge resize after the debounce window. Only touches
///    `resize_state` and `transport`.
///
/// The PTY reader OS thread does **not** touch `Terminal` directly — it sends
/// `PtyEvent::Data` through an `async_channel`, which the GPUI thread drains.
///
/// # Synchronization primitives
///
/// - **`Arc<Mutex<T>>`** — the `Arc` is needed when the value is shared with a
///   sub-struct (`ZedEventListener`, `OscSidecar`) or handed to a background
///   thread (`resize_state`). The `Mutex` (from `parking_lot`) provides
///   interior mutability.
///
/// - **`Mutex<T>`** — interior mutability for fields that don't need to be
///   shared outside the `Terminal` struct. All current `Mutex`-only fields are
///   accessed exclusively from the GPUI thread; the `Mutex` is required
///   because `&self` methods need interior mutability, not because multiple
///   threads contend.
///
/// - **`AtomicBool` / `AtomicU64`** — lock-free signaling between the GPUI
///   thread and the tokio reader task (for `dirty`), or between the GPUI
///   thread's output path and its render path (for `content_generation`,
///   `waiting_for_input`, `had_user_input`) to avoid mutex overhead on every
///   frame.
pub struct Terminal {
    // ── Immutable after construction ─────────────────────────────────

    /// Unique identifier for this terminal instance. Immutable after
    /// construction; read freely from any thread.
    pub terminal_id: String,

    /// I/O transport (local PTY or remote WebSocket). Immutable ref after
    /// construction. `Arc` for sharing with `ZedEventListener`, `OscSidecar`,
    /// and the resize debounce timer.
    pub(super) transport: Arc<dyn TerminalTransport>,

    /// Initial working directory passed at creation time. Immutable.
    /// Used as fallback when the shell has not yet reported its cwd via OSC 7.
    pub(super) initial_cwd: String,

    // ── GPUI-thread only ─────────────────────────────────────────────
    // All fields below are accessed exclusively from the GPUI thread.
    // `Mutex` provides interior mutability for `&self` methods, not
    // cross-thread safety.

    /// ANSI parser state (alacritty_terminal `Term`). Locked by
    /// `process_output`, `with_content`, `resize`, `scroll`, and selection
    /// methods — all on the GPUI thread. The `Arc` is structural: it doesn't
    /// get cloned, but `Terminal` requires `Send + Sync` and `Term` is
    /// mutated through `&self`.
    pub(super) term: Arc<Mutex<Term<ZedEventListener>>>,

    /// VTE byte processor. Locked together with `term` in `process_output`
    /// and `drain_pending_output`. GPUI thread only.
    pub(super) processor: Mutex<Processor>,

    /// Mouse/keyboard selection state. GPUI thread only (selection start,
    /// update, finish, cancel — all driven by UI events).
    pub(super) selection_state: Mutex<SelectionState>,

    /// Cumulative scroll delta in the scrollback buffer. GPUI thread only
    /// (scroll, scroll_page). The `Mutex` is for interior mutability; no
    /// cross-thread contention.
    pub(super) scroll_offset: Mutex<i32>,

    /// Terminal title set by OSC 0/1/2 sequences. `Arc` shared with
    /// `ZedEventListener` (which lives inside `Term`): the listener writes
    /// on title-change events during `process_output`, and the GPUI render
    /// path reads via `get_title`. Both happen on the GPUI thread.
    pub(super) title: Arc<Mutex<Option<String>>>,

    /// Bell notification flag. `Arc` shared with `ZedEventListener`: set on
    /// BEL during `process_output`, cleared by the render path on focus.
    /// GPUI thread only.
    pub(super) has_bell: Arc<Mutex<bool>>,

    /// One-shot "the bell rang since last drain" edge. `Arc` shared with
    /// `ZedEventListener`: set on BEL alongside `has_bell`, consumed (swapped
    /// to false) by the PTY event loop so a bell raises a desktop notification
    /// exactly once instead of on every batch while `has_bell` stays set.
    pub(super) bell_pending: Arc<AtomicBool>,
    pub(super) bell_state: Arc<Mutex<event_listener::BellRuntimeState>>,

    /// Sticky "this pane raised a desktop notification" flag, mirroring
    /// `has_bell` but for OSC 9/777 alerts. Set by the app when it actually
    /// fires a notification (so it already honors the user's settings and the
    /// focused-pane suppression); drives the pane's attention border; cleared
    /// on focus. Not shared with the listener — GPUI thread only.
    pub(super) has_notification: AtomicBool,

    /// Pending OSC 52 clipboard writes requested by the running app. `Arc`
    /// shared with `ZedEventListener`: pushed during `process_output`,
    /// drained by the GPUI render path via `drain_clipboard_writes`.
    /// GPUI thread only.
    pub(super) pending_clipboard: Arc<Mutex<Vec<String>>>,

    /// Terminal palette used to answer OSC 10/11/12/4 color queries from
    /// terminal apps. `Arc` shared with `ZedEventListener`: the render path
    /// pushes the current palette via `push_terminal_palette`, and the listener reads
    /// it when composing color-query responses. GPUI thread only.
    pub(super) palette: Arc<Mutex<Option<velowork_core::theme::TerminalPalette>>>,

    /// Working directory most recently reported by the shell via OSC 7.
    /// `None` until the shell sends its first `ESC ] 7 ; file://...`
    /// sequence. `Arc` shared with `OscSidecar` (the sidecar writes on
    /// parse, GPUI reads via `reported_cwd`). GPUI thread only.
    pub(super) reported_cwd: Arc<Mutex<Option<String>>>,

    /// Pending `OSC 9` / `OSC 777` desktop notifications. `Arc` shared with
    /// `OscSidecar`: pushed during `process_output`, drained by the GPUI
    /// thread in the PTY event loop via `take_pending_notifications`. GPUI
    /// thread only.
    pub(super) pending_notifications: Arc<Mutex<Vec<TerminalNotification>>>,

    /// Per-renderer focus state for DEC focus reports. A terminal can appear
    /// in multiple windows, so focus reports are derived from the aggregate
    /// instead of whichever view rendered last.
    focus_report_state: Mutex<FocusReportState>,

    /// VTE sidecar parser for OSC/CSI sequences (OSC 7 cwd, OSC 9
    /// notifications, XTVERSION) that alacritty_terminal either ignores or
    /// answers differently than Velowork wants. GPUI thread only
    /// (`process_output` and `drain_pending_output`).
    pub(super) osc_sidecar: Mutex<OscSidecar>,

    /// Byte-splitting sidecar for OSC 133 shell-integration marks. Runs
    /// in lockstep with the main `processor` so cursor positions can be
    /// snapshotted at the exact byte each mark arrives. GPUI thread only.
    pub(super) prompt_sidecar: Mutex<PromptSidecar>,

    /// Ring buffer of captured OSC 133 prompt marks. Written during
    /// `process_output`, read by `prompt_marks` and `jump_to_prompt_*`.
    /// GPUI thread only.
    pub(super) prompt_tracker: Mutex<PromptTracker>,

    /// One-shot "a command finished (OSC 133 ;D) since last drain" edge.
    /// Set in `process_output` when the prompt sidecar records a
    /// `CommandFinished` mark, consumed (swapped to false) by the PTY event
    /// loop so a finished command bumps the owning project's activity
    /// timestamp exactly once. Mirrors `bell_pending`; not Arc-shared since it
    /// is only ever set on the GPUI thread. GPUI thread only.
    pub(super) command_finished_pending: AtomicBool,

    /// Reverse index into the current list of `PromptStart` marks (0 =
    /// newest). `Some` while the user is walking through prompts with
    /// `jump_to_prompt_above/below`; reset to `None` on any output or
    /// scroll so the next walk starts from the most recent prompt again.
    /// GPUI thread only.
    pub(super) prompt_jump_index: Mutex<Option<usize>>,

    /// Shell process PID. Set by `set_shell_pid` (called from GPUI thread
    /// after PTY spawn), read by `shell_pid` and `has_running_child`.
    /// GPUI thread only.
    pub(super) shell_pid: Mutex<Option<u32>>,

    /// Timestamp of when the user last viewed this terminal (set on blur
    /// via `mark_as_viewed`). Compared against `last_output_time` to
    /// determine `has_unseen_output`. GPUI thread only.
    ///
    /// The `Arc` is historical — the value is never cloned; a plain `Mutex`
    /// would suffice.
    pub(super) last_viewed_time: Arc<Mutex<Instant>>,

    // ── GPUI + resize debounce timer ─────────────────────────────────

    /// Terminal size, debounce state, and pending PTY resize. `Arc` is
    /// required: a clone is handed to the short-lived debounce timer thread
    /// (`std::thread::spawn` in `resize`) which flushes the trailing-edge
    /// resize after the debounce window.
    pub resize_state: Arc<Mutex<ResizeState>>,

    // ── Cross-thread (GPUI + tokio reader task) ──────────────────────
    // These fields are touched by the remote-connection tokio reader task
    // via `enqueue_output`. The tokio task buffers data and sets flags;
    // the GPUI thread drains and clears them.

    /// Buffer for remote-connection output. Written by the tokio reader
    /// task (`enqueue_output`), drained by the GPUI thread
    /// (`drain_pending_output` inside `with_content`). Decouples the tokio
    /// task from `term.lock()`, preventing lock contention that would
    /// freeze the UI.
    pub(super) pending_output: Mutex<Vec<u8>>,

    /// Content-changed flag. Set by `process_output` (GPUI) and
    /// `enqueue_output` (tokio). Cleared by `take_dirty` (GPUI render).
    /// `AtomicBool` for lock-free cross-thread signaling.
    pub(super) dirty: AtomicBool,

    /// Timestamp of last terminal output. Written by `process_output`
    /// (GPUI), `enqueue_output` (tokio), and `clear_waiting` (GPUI). Read
    /// by idle-detection methods on the GPUI thread.
    ///
    /// The `Arc` is historical — the value is never cloned; a plain `Mutex`
    /// would suffice since `Terminal` is already behind `Arc`.
    pub(super) last_output_time: Arc<Mutex<Instant>>,

    // ── Atomics (lock-free render reads) ─────────────────────────────
    // These use atomics so the GPUI render path can read them without
    // taking a mutex on every frame.

    /// Monotonically-increasing counter bumped on every `process_output`,
    /// `drain_pending_output`, resize, scroll, and selection change. Used
    /// by `UrlDetector` and `SearchBar` to skip redundant work when
    /// content hasn't changed. GPUI thread only (despite being atomic —
    /// the atomic avoids locking, not cross-thread access).
    pub(super) content_generation: AtomicU64,

    /// Cached "waiting for input" state. Written by the GPUI idle-check
    /// loop (`set_waiting_for_input`), read lock-free by renderers
    /// (`is_waiting_for_input`). Atomic avoids mutex overhead in the
    /// render hot path.
    pub(super) waiting_for_input: AtomicBool,

    /// Whether the user has ever typed into this terminal. Set on
    /// `send_input` / `send_paste` / `send_raw_input` (GPUI thread), read
    /// lock-free by the idle-detection loop. Prevents flagging fresh
    /// terminals as idle before the user has interacted.
    pub(super) had_user_input: AtomicBool,

    /// IME composition text (marked text) for display at the cursor position.
    pub(super) marked_text: Mutex<Option<String>>,

    /// Active log recording file, path, and ANSI filter
    pub log_recording: Arc<Mutex<Option<(std::fs::File, std::path::PathBuf, log_filter::PtyLogFilter)>>>,
    /// Active log recording status (recording, paused)
    pub log_recording_paused: Arc<AtomicBool>,
    /// Active log recording start time
    pub log_recording_start_instant: Arc<Mutex<Option<std::time::Instant>>>,
    /// Accumulated seconds when paused
    pub log_recording_accumulated_secs: Arc<std::sync::atomic::AtomicU64>,
    pub(super) zmodem_detector: Mutex<crate::zmodem::ZmodemDetector>,
    pub(super) pending_upload_files: Arc<Mutex<Vec<std::path::PathBuf>>>,
    pub(super) pending_zmodem_events: Mutex<Vec<ZmodemEvent>>,
    pub(super) zmodem_raw_sender: Arc<Mutex<Option<tokio::sync::mpsc::Sender<Vec<u8>>>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZmodemEvent {
    UploadRequested,
    DownloadRequested,
}

impl Terminal {
    /// Start recording terminal output to the given file path
    pub fn start_log_recording(&self, path: std::path::PathBuf, append_mode: bool) -> std::io::Result<()> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append_mode)
            .truncate(!append_mode)
            .open(&path)?;
        *self.log_recording.lock() = Some((file, path, log_filter::PtyLogFilter::new()));
        self.log_recording_paused.store(false, std::sync::atomic::Ordering::Relaxed);
        *self.log_recording_start_instant.lock() = Some(std::time::Instant::now());
        self.log_recording_accumulated_secs.store(0, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// Set or clear the raw byte stream channel for an active ZMODEM session
    pub fn set_zmodem_raw_sender(&self, sender: Option<tokio::sync::mpsc::Sender<Vec<u8>>>) {
        *self.zmodem_raw_sender.lock() = sender;
    }

    /// Abort an in-progress ZMODEM file transfer by sending 5x CAN + 5x BS
    pub fn cancel_zmodem(&self) {
        self.set_zmodem_raw_sender(None);
        self.send_bytes(b"\x18\x18\x18\x18\x18\x08\x08\x08\x08\x08");
    }

    /// Stop recording terminal output and return the path of the saved log file
    pub fn stop_log_recording(&self) -> Option<std::path::PathBuf> {
        let mut recording = self.log_recording.lock();
        let opt = recording.take();
        self.log_recording_paused.store(false, std::sync::atomic::Ordering::Relaxed);
        *self.log_recording_start_instant.lock() = None;
        self.log_recording_accumulated_secs.store(0, std::sync::atomic::Ordering::Relaxed);
        opt.map(|(_, path, _)| path)
    }

    /// Pause log recording
    pub fn pause_log_recording(&self) {
        self.log_recording_paused.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(start) = self.log_recording_start_instant.lock().take() {
            let elapsed = start.elapsed().as_secs();
            let mut accum = self.log_recording_accumulated_secs.load(std::sync::atomic::Ordering::Relaxed);
            accum += elapsed;
            self.log_recording_accumulated_secs.store(accum, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Resume log recording
    pub fn resume_log_recording(&self) {
        self.log_recording_paused.store(false, std::sync::atomic::Ordering::Relaxed);
        *self.log_recording_start_instant.lock() = Some(std::time::Instant::now());
    }

    /// Check if log recording is active (whether we have an active file open)
    pub fn is_log_recording(&self) -> bool {
        self.log_recording.lock().is_some()
    }

    /// Check if log recording is paused
    pub fn is_log_recording_paused(&self) -> bool {
        self.log_recording_paused.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get the current log recording file path
    pub fn get_log_recording_path(&self) -> Option<std::path::PathBuf> {
        self.log_recording.lock().as_ref().map(|(_, path, _)| path.clone())
    }

    /// Flush current log recording file
    pub fn flush_log(&self) {
        if let Some((file, _, _)) = &mut *self.log_recording.lock() {
            use std::io::Write;
            let _ = file.flush();
        }
    }

    /// Get elapsed seconds since log recording started
    pub fn get_log_recording_elapsed_secs(&self) -> u64 {
        let mut total = self.log_recording_accumulated_secs.load(std::sync::atomic::Ordering::Relaxed);
        if let Some(start) = *self.log_recording_start_instant.lock() {
            total += start.elapsed().as_secs();
        }
        total
    }

    /// Create a new terminal with default scrollback lines (10000)
    pub fn new(
        terminal_id: String,
        size: TerminalSize,
        transport: Arc<dyn TerminalTransport>,
        initial_cwd: String,
    ) -> Self {
        Self::new_with_scrollback(terminal_id, size, transport, initial_cwd, 10000)
    }

    /// Create a new terminal with custom scrollback lines
    pub fn new_with_scrollback(
        terminal_id: String,
        size: TerminalSize,
        transport: Arc<dyn TerminalTransport>,
        initial_cwd: String,
        scrollback_lines: usize,
    ) -> Self {
        Self::new_with_options(terminal_id, size, transport, initial_cwd, scrollback_lines, None)
    }

    /// Create a new terminal with custom scrollback lines and word separators (semantic escape chars)
    pub fn new_with_options(
        terminal_id: String,
        size: TerminalSize,
        transport: Arc<dyn TerminalTransport>,
        initial_cwd: String,
        scrollback_lines: usize,
        word_separators: Option<String>,
    ) -> Self {
        // Use HollowBlock as a sentinel for "app has not set a cursor shape
        // via DECSCUSR" — no real DECSCUSR code maps to HollowBlock, so if
        // `cursor_style()` returns it we know to fall back to the user
        // setting instead of honoring an app override.
        let config = TermConfig {
            default_cursor_style: VteCursorStyle {
                shape: VteCursorShape::HollowBlock,
                blinking: false,
            },
            scrolling_history: scrollback_lines,
            semantic_escape_chars: word_separators
                .unwrap_or_else(|| " `/\\()\"':,.;<>~!@#$%^&*|+=[]{}`~?".to_string()),
            ..TermConfig::default()
        };
        let term_size = TermSize::new(size.cols as usize, size.rows as usize);

        // Create shared storage for OSC sequence handling and bell
        let title = Arc::new(Mutex::new(None));
        let has_bell = Arc::new(Mutex::new(false));
        let bell_pending = Arc::new(AtomicBool::new(false));
        let bell_state = Arc::new(Mutex::new(event_listener::BellRuntimeState::default()));
        let pending_clipboard = Arc::new(Mutex::new(Vec::new()));
        let palette = Arc::new(Mutex::new(None));
        let event_listener = ZedEventListener::new(
            title.clone(),
            has_bell.clone(),
            bell_pending.clone(),
            bell_state.clone(),
            pending_clipboard.clone(),
            palette.clone(),
            transport.clone(),
            terminal_id.clone(),
        );
        let term = Term::new(config, &term_size, event_listener);

        let reported_cwd = Arc::new(Mutex::new(None));
        let pending_notifications = Arc::new(Mutex::new(Vec::new()));
        let osc_sidecar = Mutex::new(OscSidecar::new(
            reported_cwd.clone(),
            pending_notifications.clone(),
            transport.clone(),
            terminal_id.clone(),
        ));

        Self {
            term: Arc::new(Mutex::new(term)),
            processor: Mutex::new(Processor::new()),
            terminal_id,
            resize_state: Arc::new(Mutex::new(ResizeState::new(size))),
            transport,
            selection_state: Mutex::new(SelectionState::default()),
            scroll_offset: Mutex::new(0),
            title,
            has_bell,
            bell_pending,
            bell_state,
            has_notification: AtomicBool::new(false),
            pending_clipboard,
            palette,
            pending_output: Mutex::new(Vec::new()),
            dirty: AtomicBool::new(false),
            content_generation: AtomicU64::new(0),
            initial_cwd,
            reported_cwd,
            pending_notifications,
            focus_report_state: Mutex::new(FocusReportState::default()),
            osc_sidecar,
            prompt_sidecar: Mutex::new(PromptSidecar::new()),
            prompt_tracker: Mutex::new(PromptTracker::new()),
            command_finished_pending: AtomicBool::new(false),
            prompt_jump_index: Mutex::new(None),
            last_output_time: Arc::new(Mutex::new(Instant::now())),
            shell_pid: Mutex::new(None),
            waiting_for_input: AtomicBool::new(false),
            had_user_input: AtomicBool::new(false),
            marked_text: Mutex::new(None),
            last_viewed_time: Arc::new(Mutex::new(Instant::now())),
            log_recording: Arc::new(Mutex::new(None)),
            log_recording_paused: Arc::new(AtomicBool::new(false)),
            log_recording_start_instant: Arc::new(Mutex::new(None)),
            log_recording_accumulated_secs: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            zmodem_detector: Mutex::new(crate::zmodem::ZmodemDetector::new()),
            pending_upload_files: Arc::new(Mutex::new(Vec::new())),
            pending_zmodem_events: Mutex::new(Vec::new()),
            zmodem_raw_sender: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_pending_upload_files(&self, files: Vec<std::path::PathBuf>) {
        *self.pending_upload_files.lock() = files;
    }

    pub fn take_pending_upload_files(&self) -> Vec<std::path::PathBuf> {
        std::mem::take(&mut *self.pending_upload_files.lock())
    }

    pub fn take_zmodem_events(&self) -> Vec<ZmodemEvent> {
        std::mem::take(&mut *self.pending_zmodem_events.lock())
    }

    /// Get current IME composition text.
    pub fn marked_text(&self) -> Option<String> {
        self.marked_text.lock().clone()
    }

    /// Set current IME composition text and mark terminal content as dirty.
    pub fn set_marked_text(&self, text: Option<String>) {
        *self.marked_text.lock() = text;
        self.dirty.store(true, std::sync::atomic::Ordering::Release);
    }

    /// Push current terminal palette for answering OSC 4/10/11/12 color queries.
    pub fn push_terminal_palette(&self, palette: velowork_core::theme::TerminalPalette) {
        *self.palette.lock() = Some(palette);
    }

    /// Get current effective terminal palette if set.
    pub fn palette(&self) -> Option<velowork_core::theme::TerminalPalette> {
        *self.palette.lock()
    }

    /// Update effective runtime bell configuration (dirty-checked, no unnecessary lock contention).
    pub fn update_bell_config(&self, style: velowork_core::types::BellStyle, cooldown_ms: u32) {
        let mut state = self.bell_state.lock();
        if state.style != style || state.cooldown_ms != cooldown_ms {
            state.style = style;
            state.cooldown_ms = cooldown_ms;
        }
    }
}
