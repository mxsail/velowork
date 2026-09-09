use super::super::Terminal;
use super::super::types::TerminalSize;
use super::NullTransport;
use std::sync::Arc;

#[test]
fn test_terminal_preview_snapshot_empty() {
    let transport = Arc::new(NullTransport);
    let terminal = Terminal::new(
        "test-term".into(),
        TerminalSize::default(),
        transport,
        "/tmp".into(),
    );

    let snapshot = terminal.preview_snapshot(16);
    assert!(snapshot.is_empty);
    assert!(snapshot.lines.is_empty());
}

#[test]
fn test_terminal_preview_snapshot_with_content() {
    let transport = Arc::new(NullTransport);
    let terminal = Terminal::new(
        "test-term".into(),
        TerminalSize::default(),
        transport,
        "/tmp".into(),
    );

    terminal.process_output(b"Hello, Velowork!\r\n\x1b[32mGreen Text\x1b[0m\r\n");

    let snapshot = terminal.preview_snapshot(16);
    assert!(!snapshot.is_empty);
    assert!(snapshot.lines.len() >= 2);

    let first_line_text = snapshot.lines[0]
        .spans
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert!(first_line_text.contains("Hello, Velowork!"));

    let second_line_has_green = snapshot.lines[1]
        .spans
        .iter()
        .any(|s| s.fg.is_some() && s.text.contains("Green Text"));
    assert!(second_line_has_green);
}
