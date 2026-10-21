use std::sync::Arc;
use crate::terminal::tests::helpers::CapturingTransport;
use crate::terminal::{Terminal, TerminalSize};

#[test]
fn test_cancel_zmodem_sends_abort_sequence_and_cleans_state() {
    let transport = Arc::new(CapturingTransport::new());
    let terminal = Terminal::new(
        "test-term".into(),
        TerminalSize::default(),
        transport.clone(),
        "/tmp".into(),
    );

    // Simulate ZMODEM active state
    terminal.set_zmodem_active(true);
    assert!(terminal.is_zmodem_active());

    // Write cancellation message to screen (should not deadlock)
    terminal.write_to_screen(b"\r\nTransfer cancelled.\r\n");

    // Cancel ZMODEM
    terminal.cancel_zmodem();

    // Verify state cleanup
    assert!(!terminal.is_zmodem_active());
    assert!(terminal.is_zmodem_suppressed());

    // Verify abort bytes sent to transport
    let writes = transport.writes();
    assert!(!writes.is_empty(), "Abort sequence must be sent to transport");
    let combined: Vec<u8> = writes.concat();
    assert!(
        combined.windows(10).any(|w| w == crate::zmodem::session::ZMODEM_CANCEL_SEQUENCE),
        "Abort sequence must contain 10x CAN"
    );
}

#[test]
fn test_write_to_screen_concurrent_with_process_output_no_deadlock() {
    let transport = Arc::new(CapturingTransport::new());
    let terminal = Arc::new(Terminal::new(
        "test-concurrent-term".into(),
        TerminalSize::default(),
        transport,
        "/tmp".into(),
    ));

    let term1 = terminal.clone();
    let term2 = terminal.clone();

    let h1 = std::thread::spawn(move || {
        for i in 0..100 {
            term1.write_to_screen(format!("Local status message {}\r\n", i).as_bytes());
        }
    });

    let h2 = std::thread::spawn(move || {
        for i in 0..100 {
            term2.process_output(format!("Remote output data {}\r\n", i).as_bytes());
        }
    });

    h1.join().expect("h1 join must succeed");
    h2.join().expect("h2 join must succeed");
}

#[test]
fn test_cancel_zmodem_pre_transfer_leaves_no_swallow_window() {
    let transport = Arc::new(CapturingTransport::new());
    let terminal = Terminal::new(
        "test-term-pre".into(),
        TerminalSize::default(),
        transport.clone(),
        "/tmp".into(),
    );

    // Simulate ZMODEM active state before user selected files
    terminal.set_zmodem_active(true);
    assert!(terminal.is_zmodem_active());

    // Pre-transfer cancel (e.g. user clicked Cancel on file dialog)
    terminal.cancel_zmodem_pre_transfer();

    // Verify active state cleared
    assert!(!terminal.is_zmodem_active());
    // Suppression active to avoid duplicate dialog triggers
    assert!(terminal.is_zmodem_suppressed());
    // Swallow window MUST NOT be active so shell prompt is never swallowed!
    assert!(!terminal.is_zmodem_swallowing());

    // Verify abort sequence was sent
    let writes = transport.writes();
    assert!(!writes.is_empty());
    let combined: Vec<u8> = writes.concat();
    assert!(
        combined.windows(10).any(|w| w == crate::zmodem::session::ZMODEM_CANCEL_SEQUENCE),
        "Abort sequence must contain 10x CAN"
    );

    // Verify incoming prompt from remote shell is NOT swallowed
    terminal.process_output(b"user@host:~$ ");
    terminal.inspect_term(|term| {
        let content: String = term.grid().display_iter().map(|c| c.c).collect();
        assert!(content.contains("user@host"), "Shell prompt must be rendered immediately");
    });
}
