use std::path::PathBuf;
use std::sync::Arc;
use crate::terminal::tests::helpers::NullTransport;
use crate::terminal::{Terminal, TerminalSize};

#[test]
fn test_process_output_missing_rz_no_deadlock() {
    let terminal = Terminal::new(
        "test-term".into(),
        TerminalSize::default(),
        Arc::new(NullTransport),
        "/tmp".into(),
    );

    terminal.set_pending_upload_files(vec![PathBuf::from("/tmp/foo.txt")]);
    assert!(terminal.has_pending_upload_files());

    // When remote outputs "zsh: command not found: rz", process_output must:
    // 1. Not deadlock
    // 2. Clear pending upload files so no timeout watchdog fires
    // 3. Render the remote shell's own output naturally to screen without extra injection
    terminal.process_output(b"zsh: command not found: rz\r\n");

    assert!(!terminal.has_pending_upload_files());
    let events = terminal.take_zmodem_events();
    assert!(events.is_empty());
}
