use super::super::Terminal;
use super::super::types::TerminalSize;
use super::NullTransport;
use std::sync::Arc;
use std::fs;

#[test]
fn test_terminal_log_recording() {
    let transport = Arc::new(NullTransport);
    let terminal = Terminal::new(
        "test-id".to_string(),
        TerminalSize::default(),
        transport,
        "/tmp".to_string(),
    );

    // Create a temporary file path
    let temp_dir = std::env::temp_dir();
    let log_path = temp_dir.join("test-terminal-log.log");
    if log_path.exists() {
        let _ = fs::remove_file(&log_path);
    }

    assert!(!terminal.is_log_recording());

    // Start recording
    let success = terminal.start_log_recording(log_path.clone(), false);
    assert!(success.is_ok());
    assert!(terminal.is_log_recording());
    assert!(!terminal.is_log_recording_paused());

    // Feed some output data
    let test_data = b"hello world\r\n";
    terminal.process_output(test_data);

    // Flush and verify content
    terminal.flush_log();
    let content = fs::read_to_string(&log_path).unwrap();
    assert_eq!(content, "hello world\r\n");

    // Pause recording
    terminal.pause_log_recording();
    assert!(terminal.is_log_recording_paused());

    // Feed some output data during pause
    terminal.process_output(b"should not be recorded\r\n");
    terminal.flush_log();
    let content = fs::read_to_string(&log_path).unwrap();
    assert_eq!(content, "hello world\r\n"); // unchanged

    // Resume recording
    terminal.resume_log_recording();
    assert!(!terminal.is_log_recording_paused());

    // Feed some output data after resume
    terminal.process_output(b"resumed recording\r\n");
    terminal.flush_log();
    let content = fs::read_to_string(&log_path).unwrap();
    assert_eq!(content, "hello world\r\nresumed recording\r\n");

    // Stop recording
    let saved_path = terminal.stop_log_recording();
    assert_eq!(saved_path, Some(log_path.clone()));
    assert!(!terminal.is_log_recording());

    // Clean up
    let _ = fs::remove_file(&log_path);
}

#[test]
fn test_terminal_log_recording_stop_while_paused() {
    let transport = Arc::new(NullTransport);
    let terminal = Terminal::new(
        "test-id".to_string(),
        TerminalSize::default(),
        transport,
        "/tmp".to_string(),
    );

    let temp_dir = std::env::temp_dir();
    let log_path = temp_dir.join("test-terminal-log-paused.log");
    if log_path.exists() {
        let _ = fs::remove_file(&log_path);
    }

    assert!(!terminal.is_log_recording());
    let success = terminal.start_log_recording(log_path.clone(), false);
    assert!(success.is_ok());
    assert!(terminal.is_log_recording());

    // Pause recording
    terminal.pause_log_recording();
    assert!(terminal.is_log_recording_paused());

    // Stop recording while paused
    let saved_path = terminal.stop_log_recording();
    assert_eq!(saved_path, Some(log_path.clone()));
    assert!(!terminal.is_log_recording());

    if log_path.exists() {
        let _ = fs::remove_file(&log_path);
    }
}
