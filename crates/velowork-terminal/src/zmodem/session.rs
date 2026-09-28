//! ZMODEM file transfer session helper routines for upload (rz) and download (sz).
//!
//! Compliant with lrzsz and standard ZMODEM protocol specifications.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::UnboundedReceiver;

use super::protocol::*;

/// 10x CAN (0x18) to reliably exceed lrzsz 5-CAN threshold and cleanly cancel remote ZMODEM without interleaved characters.
pub const ZMODEM_CANCEL_SEQUENCE: &[u8] = b"\x18\x18\x18\x18\x18\x18\x18\x18\x18\x18";

/// Abort an active ZMODEM session and drain in-flight binary packets from `raw_rx`
/// so that no garbled characters leak into the terminal emulator.
pub async fn abort_and_drain_stream(
    terminal: &crate::terminal::Terminal,
    raw_rx: &mut UnboundedReceiver<Vec<u8>>,
) {
    log::info!("[ZMODEM-SESSION] abort_and_drain_stream: sending abort sequence");
    let mut abort_bytes = Vec::with_capacity(40);
    // 10x CAN + 10x BS: standard ZMODEM protocol abort sequence recognized by lrzsz
    abort_bytes.extend_from_slice(ZMODEM_CANCEL_SEQUENCE);
    abort_bytes.extend_from_slice(b"\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08");
    // Request prompt reset with Ctrl+C and newline
    abort_bytes.extend_from_slice(b"\r\n\x03\x03\r\n");
    terminal.send_bytes(&abort_bytes);

    // Suppress detector temporarily so trailing abort frames don't re-trigger sessions
    terminal.suppress_zmodem_temporarily(std::time::Duration::from_millis(1500));

    // Drain in-flight bytes from raw_rx.
    // Because zmodem_raw_sender is still registered, all incoming chunks from remote
    // will be delivered to raw_rx and discarded here, preventing any screen leakage.
    let drain_deadline = Instant::now() + std::time::Duration::from_millis(800);
    while Instant::now() < drain_deadline {
        match tokio::time::timeout(std::time::Duration::from_millis(150), raw_rx.recv()).await {
            Ok(Some(chunk)) => {
                // Remote lsz/lrz sends 5x ZDLE (CAN) when it exits on abort
                if chunk.windows(5).any(|w| w == [ZDLE; 5]) {
                    log::info!("[ZMODEM-SESSION] Remote acknowledged abort with CAN sequence");
                    break;
                }
            }
            _ => {
                // Quiet link or channel closed: remote has finished transmitting
                break;
            }
        }
    }
    log::info!("[ZMODEM-SESSION] abort_and_drain_stream: drain completed");
}

/// Generate a unique local download path to avoid overwriting existing files.
/// E.g. `foo.tar.gz` -> `foo (1).tar.gz` if `foo.tar.gz` exists.
pub fn resolve_unique_download_path(dir: &Path, filename: &str) -> PathBuf {
    let base_path = dir.join(filename);
    if !base_path.exists() {
        return base_path;
    }

    let p = Path::new(filename);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or(filename);
    let extension = p.extension().and_then(|e| e.to_str());

    let mut counter = 1;
    loop {
        let new_name = match extension {
            Some(ext) => format!("{} ({}).{}", stem, counter, ext),
            None => format!("{} ({})", stem, counter),
        };
        let candidate = dir.join(&new_name);
        if !candidate.exists() {
            return candidate;
        }
        counter += 1;
    }
}

/// Build a `ZFILE` data payload containing `filename\0size mtime...`
pub fn build_zfile_payload(filename: &str, file_size: u64) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(filename.as_bytes());
    payload.push(0); // null terminator

    let meta = format!("{} 0", file_size);
    payload.extend_from_slice(meta.as_bytes());
    payload.push(0); // null terminator

    payload
}

/// Parse filename and file size from a `ZFILE` data payload
pub fn parse_zfile_payload(payload: &[u8]) -> Option<(String, u64)> {
    let null_pos = payload.iter().position(|&b| b == 0)?;
    let filename = std::str::from_utf8(&payload[..null_pos]).ok()?.to_string();

    let rest = &payload[null_pos + 1..];
    let second_null = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
    let meta_str = std::str::from_utf8(&rest[..second_null]).ok()?;
    let file_size = meta_str
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    Some((filename, file_size))
}

/// Explicit classification for why a file was skipped by the remote receiver (rz).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZmodemSkipReason {
    /// Remote destination file is protected (e.g. read-only file mode, write permission denied, or remote receiver protect policy `-p`).
    Protected,
    /// Destination file already exists on the remote host and overwrite was not requested.
    FileExists,
}

impl ZmodemSkipReason {
    /// Detailed description in Chinese and English.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Protected => "服务端目标文件受保护或无写入权限，禁止修改 (Protected: Permission denied)",
            Self::FileExists => "服务端已存在同名文件 (Skipped: File already exists)",
        }
    }

    /// Short label
    pub fn label(&self) -> &'static str {
        match self {
            Self::Protected => "受保护 (Protected)",
            Self::FileExists => "已跳过 (Skipped)",
        }
    }
}

/// Summary of an upload batch session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ZmodemUploadSummary {
    pub total_files: usize,
    pub transferred_files: usize,
    pub transferred_bytes: u64,
    pub skipped_files: Vec<(String, ZmodemSkipReason)>,
}

/// Events emitted during upload progress and lifecycle
#[derive(Debug, Clone, PartialEq)]
pub enum ZmodemUploadEvent {
    Progress {
        transferred: u64,
        total: u64,
        speed: f64,
        filename: String,
        file_idx: usize,
        total_files: usize,
    },
    FileSkipped {
        filename: String,
        reason: ZmodemSkipReason,
    },
}

/// Send a local file (or batch of files) over ZMODEM protocol with full handshaking and progress updates.
pub async fn send_zmodem_upload_with_progress<F>(
    terminal: Arc<crate::terminal::Terminal>,
    local_paths: Vec<PathBuf>,
    overwrite: bool,
    mut raw_rx: UnboundedReceiver<Vec<u8>>,
    cancel_flag: Arc<AtomicBool>,
    mut event_cb: F,
) -> Result<ZmodemUploadSummary, String>
where
    F: FnMut(ZmodemUploadEvent) + Send + 'static,
{
    let total_files = local_paths.len();
    if total_files == 0 {
        return Ok(ZmodemUploadSummary::default());
    }
    log::info!("[ZMODEM-SESSION] Starting upload | total_files={} | overwrite={}", total_files, overwrite);

    let mut stream_buffer = Vec::with_capacity(32768);
    let timeout = std::time::Duration::from_secs(30);
    let mut total_transferred_bytes: u64 = 0;
    let mut transferred_files_count: usize = 0;
    let mut skipped_files: Vec<(String, ZmodemSkipReason)> = Vec::new();

    for (file_idx, local_path) in local_paths.into_iter().enumerate() {
        if cancel_flag.load(Ordering::Relaxed) {
            log::info!("[ZMODEM-SESSION] Upload cancelled by user, draining stream...");
            abort_and_drain_stream(&terminal, &mut raw_rx).await;
            return Err("Upload cancelled by user".to_string());
        }

        let filename = local_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let bytes = std::fs::read(&local_path)
            .map_err(|e| format!("Failed to read file {:?}: {}", local_path, e))?;
        let file_size = bytes.len() as u64;

        // 1. Send ZFILE header (Hex header, type ZFILE)
        // Wire order in header array: [ZF3, ZF2, ZF1, ZF0]
        // ZF0 (index 3): ZCBIN (binary transfer)
        // ZF1 (index 2): ZF1_ZMCLOB (overwrite existing destination file if requested)
        let zf1 = if overwrite { ZF1_ZMCLOB } else { 0 };
        let mut zfile_flags = [0u8; 4];
        zfile_flags[ZF0_IDX] = ZCBIN;
        zfile_flags[ZF1_IDX] = zf1;
        let zfile_hdr = build_hex_header(ZFILE, zfile_flags);
        terminal.send_bytes(&zfile_hdr);

        // 2. Send ZFILE payload subpacket with ZCRCW delimiter
        let zfile_payload = build_zfile_payload(&filename, file_size);
        let zfile_subpacket = encode_subpacket(&zfile_payload, ZCRCW, false);
        terminal.send_bytes(&zfile_subpacket);

        // 3. Wait for ZRPOS from receiver
        let mut start_pos: u64 = 0;
        let mut got_zrpos = false;
        let mut file_skip_reason: Option<ZmodemSkipReason> = None;
        let wait_start = Instant::now();

        while !got_zrpos && file_skip_reason.is_none() {
            if cancel_flag.load(Ordering::Relaxed) {
                log::info!("[ZMODEM-SESSION] Upload cancelled by user, draining stream...");
                abort_and_drain_stream(&terminal, &mut raw_rx).await;
                return Err("Upload cancelled by user".to_string());
            }
            if wait_start.elapsed() > timeout {
                terminal.send_bytes(ZMODEM_CANCEL_SEQUENCE);
                return Err("Timed out waiting for ZRPOS from remote rz".to_string());
            }

            match tokio::time::timeout(std::time::Duration::from_millis(500), raw_rx.recv()).await {
                Ok(Some(chunk)) => {
                    stream_buffer.extend_from_slice(&chunk);
                    // Check for cancel sequence: 5x CAN
                    if stream_buffer.windows(5).any(|w| w == [ZDLE, ZDLE, ZDLE, ZDLE, ZDLE]) {
                        return Err("ZMODEM transfer was cancelled by remote".to_string());
                    }
                    while let Some((hdr, _start, end)) = parse_any_header(&stream_buffer) {
                        stream_buffer.drain(..end);
                        match hdr.parsed_type() {
                            ZmodemHeaderType::Zrpos(pos) => {
                                start_pos = pos as u64;
                                got_zrpos = true;
                                break;
                            }
                            ZmodemHeaderType::Zskip => {
                                let is_explicit_protect = hdr.flags[ZF1_IDX] == ZF1_ZMPROT;
                                let reason = if is_explicit_protect || overwrite {
                                    ZmodemSkipReason::Protected
                                } else {
                                    ZmodemSkipReason::FileExists
                                };
                                log::warn!(
                                    "[ZMODEM-UPLOAD] Remote rz requested to skip file: {} | reason={:?} | overwrite={} | flags={:?}",
                                    filename,
                                    reason,
                                    overwrite,
                                    hdr.flags
                                );
                                file_skip_reason = Some(reason);
                                break;
                            }
                            ZmodemHeaderType::Zcan => {
                                return Err("Remote cancelled transfer".to_string());
                            }
                            _ => {}
                        }
                    }
                }
                Ok(None) => {
                    return Err("Terminal input stream closed".to_string());
                }
                Err(_) => {
                    // Check timeout
                }
            }
        }

        if let Some(reason) = file_skip_reason {
            log::info!("[ZMODEM-UPLOAD] File {} skipped by remote: {:?}", filename, reason);
            event_cb(ZmodemUploadEvent::FileSkipped {
                filename: filename.clone(),
                reason,
            });
            skipped_files.push((filename, reason));
            continue;
        }

        // 4. Send ZDATA header at offset
        let zdata_hdr = build_binary32_header(ZDATA, (start_pos as u32).to_le_bytes());
        terminal.send_bytes(&zdata_hdr);

        // 5. Send file chunks with CRC32 streaming (Full Streaming with sliding window backpressure)
        let transfer_start = Instant::now();
        let chunk_size = 4096;
        let mut pos = (start_pos as usize).min(bytes.len());
        let mut last_acked_pos = pos;
        let max_in_flight = 64 * 1024; // 64KB window prevents unbounded PTY channel bloat
        let ack_interval = 16 * 1024; // Request ACK every 16KB
        let mut bytes_since_ack_req = 0;

        while pos < bytes.len() {
            if cancel_flag.load(Ordering::Relaxed) {
                terminal.send_bytes(ZMODEM_CANCEL_SEQUENCE);
                return Err("Upload cancelled by user".to_string());
            }

            // Check for incoming packets from receiver (ZACK / ZRPOS / Zcan / cancel sequence)
            let mut read_channel = true;
            while read_channel {
                match raw_rx.try_recv() {
                    Ok(chunk) => {
                        stream_buffer.extend_from_slice(&chunk);
                        if stream_buffer.windows(5).any(|w| w == [ZDLE, ZDLE, ZDLE, ZDLE, ZDLE]) {
                            return Err("Remote cancelled transfer".to_string());
                        }
                        while let Some((hdr, _start, consumed)) = parse_any_header(&stream_buffer) {
                            stream_buffer.drain(..consumed);
                            match hdr.parsed_type() {
                                ZmodemHeaderType::Zrpos(new_pos) => {
                                    pos = (new_pos as usize).min(bytes.len());
                                    last_acked_pos = pos;
                                    bytes_since_ack_req = 0;
                                    log::info!("[ZMODEM-UPLOAD] Remote requested retransmit from offset {}", pos);
                                }
                                ZmodemHeaderType::Zack => {
                                    let ack_pos = u32::from_le_bytes(hdr.flags) as usize;
                                    if ack_pos > last_acked_pos {
                                        last_acked_pos = ack_pos;
                                    }
                                }
                                ZmodemHeaderType::Zcan => {
                                    return Err("Remote cancelled transfer".to_string());
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                        read_channel = false;
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        return Err("Terminal stream closed during upload".to_string());
                    }
                }
            }

            // If we have more than max_in_flight unacknowledged bytes, wait for ACK or timeout
            if pos.saturating_sub(last_acked_pos) >= max_in_flight {
                if cancel_flag.load(Ordering::Relaxed) {
                    log::info!("[ZMODEM-SESSION] Upload cancelled by user, draining stream...");
                    abort_and_drain_stream(&terminal, &mut raw_rx).await;
                    return Err("Upload cancelled by user".to_string());
                }
                match tokio::time::timeout(std::time::Duration::from_millis(200), raw_rx.recv()).await {
                    Ok(Some(chunk)) => {
                        stream_buffer.extend_from_slice(&chunk);
                        if stream_buffer.windows(5).any(|w| w == [ZDLE, ZDLE, ZDLE, ZDLE, ZDLE]) {
                            return Err("Remote cancelled transfer".to_string());
                        }
                        while let Some((hdr, _start, consumed)) = parse_any_header(&stream_buffer) {
                            stream_buffer.drain(..consumed);
                            match hdr.parsed_type() {
                                ZmodemHeaderType::Zrpos(new_pos) => {
                                    pos = (new_pos as usize).min(bytes.len());
                                    last_acked_pos = pos;
                                    bytes_since_ack_req = 0;
                                    log::info!("[ZMODEM-UPLOAD] Remote requested retransmit from offset {}", pos);
                                }
                                ZmodemHeaderType::Zack => {
                                    let ack_pos = u32::from_le_bytes(hdr.flags) as usize;
                                    if ack_pos > last_acked_pos {
                                        last_acked_pos = ack_pos;
                                    }
                                }
                                ZmodemHeaderType::Zcan => {
                                    return Err("Remote cancelled transfer".to_string());
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(None) => {
                        if cancel_flag.load(Ordering::Relaxed) {
                            return Err("Upload cancelled by user".to_string());
                        } else {
                            return Err("Terminal stream closed during upload".to_string());
                        }
                    }
                    Err(_) => {
                        // Soft fallback if remote does not send ZACK for ZCRCQ
                        last_acked_pos = pos;
                    }
                }
            }

            let end = (pos + chunk_size).min(bytes.len());
            let is_last = end >= bytes.len();
            let chunk = &bytes[pos..end];
            bytes_since_ack_req += chunk.len();

            let frame_end = if is_last {
                ZCRCE
            } else if bytes_since_ack_req >= ack_interval {
                bytes_since_ack_req = 0;
                ZCRCQ
            } else {
                ZCRCG
            };

            let subpacket = encode_subpacket(chunk, frame_end, true);
            terminal.send_bytes(&subpacket);
            pos = end;

            let transferred = pos as u64;
            let elapsed = transfer_start.elapsed().as_secs_f64().max(0.001);
            let speed = transferred as f64 / elapsed;
            event_cb(ZmodemUploadEvent::Progress {
                transferred,
                total: file_size,
                speed,
                filename: filename.clone(),
                file_idx: file_idx + 1,
                total_files,
            });

            tokio::task::yield_now().await;
        }

        // 6. Send ZEOF header
        let zeof_hdr = build_binary32_header(ZEOF, (file_size as u32).to_le_bytes());
        terminal.send_bytes(&zeof_hdr);

        // 7. Wait for ZRINIT from remote rz acknowledging file completion
        let mut got_zrinit = false;
        let eof_start = Instant::now();
        while !got_zrinit && eof_start.elapsed() < timeout {
            if cancel_flag.load(Ordering::Relaxed) {
                terminal.send_bytes(ZMODEM_CANCEL_SEQUENCE);
                return Err("Upload cancelled by user".to_string());
            }
            match tokio::time::timeout(std::time::Duration::from_millis(500), raw_rx.recv()).await {
                Ok(Some(chunk)) => {
                    stream_buffer.extend_from_slice(&chunk);
                    while let Some((hdr, _start, consumed)) = parse_any_header(&stream_buffer) {
                        stream_buffer.drain(..consumed);
                        if let ZmodemHeaderType::Zrinit = hdr.parsed_type() {
                            got_zrinit = true;
                            break;
                        }
                    }
                }
                Ok(None) => {
                    return Err("Terminal stream closed during upload".to_string());
                }
                Err(_) => {
                    // Timeout (500ms) - continue checking elapsed
                }
            }
        }
        total_transferred_bytes += file_size;
        transferred_files_count += 1;
    }

    // 8. End session: send ZFIN
    let zfin_hdr = build_hex_header(ZFIN, [0, 0, 0, 0]);
    terminal.send_bytes(&zfin_hdr);

    // Wait for remote ZFIN response
    let fin_start = Instant::now();
    let mut got_zfin = false;
    while !got_zfin && fin_start.elapsed() < std::time::Duration::from_secs(5) {
        match tokio::time::timeout(std::time::Duration::from_millis(200), raw_rx.recv()).await {
            Ok(Some(chunk)) => {
                stream_buffer.extend_from_slice(&chunk);
                while let Some((hdr, _start, consumed)) = parse_any_header(&stream_buffer) {
                    stream_buffer.drain(..consumed);
                    if let ZmodemHeaderType::Zfin = hdr.parsed_type() {
                        got_zfin = true;
                        break;
                    }
                }
            }
            Ok(None) | Err(_) => {
                break;
            }
        }
    }

    // 9. Send Over-and-Out "OO"
    terminal.send_bytes(b"OO\r");
    log::info!(
        "[ZMODEM-SESSION] Upload session completed | transferred={}/{} files ({} bytes) | skipped={}",
        transferred_files_count,
        total_files,
        total_transferred_bytes,
        skipped_files.len()
    );
    Ok(ZmodemUploadSummary {
        total_files,
        transferred_files: transferred_files_count,
        transferred_bytes: total_transferred_bytes,
        skipped_files,
    })
}

/// Receive downloaded files from remote via ZMODEM (`sz`) protocol.
pub async fn receive_zmodem_download_with_progress<F>(
    terminal: Arc<crate::terminal::Terminal>,
    target_dir: PathBuf,
    mut raw_rx: UnboundedReceiver<Vec<u8>>,
    cancel_flag: Arc<AtomicBool>,
    mut progress_cb: F,
) -> Result<PathBuf, String>
where
    F: FnMut(u64, u64, f64, &str, usize, usize) + Send + 'static,
{
    use std::io::Write;
    log::info!("[ZMODEM-SESSION] Starting download into {:?}", target_dir);

    // 1. Send ZRINIT to acknowledge sz's ZRQINIT
    let zrinit_hdr = build_hex_header(ZRINIT, [0, 0, 0, CANFDX | CANOVIO | CANFC32]);
    terminal.send_bytes(&zrinit_hdr);

    struct IncompleteFileGuard(Option<(std::fs::File, PathBuf, u64, u64, Instant)>);
    impl Drop for IncompleteFileGuard {
        fn drop(&mut self) {
            if let Some((f, path, _, _, _)) = self.0.take() {
                drop(f);
                let _ = std::fs::remove_file(path);
            }
        }
    }

    let mut current_file = IncompleteFileGuard(None);
    let mut stream_buffer = Vec::with_capacity(65536);
    let mut last_saved_path = target_dir.clone();
    let mut file_idx = 0usize;

    let timeout = std::time::Duration::from_secs(45);

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            log::info!("[ZMODEM-SESSION] Download cancelled by user, draining in-flight bytes...");
            abort_and_drain_stream(&terminal, &mut raw_rx).await;
            return Err("Download cancelled by user".to_string());
        }

        let chunk_opt = tokio::time::timeout(timeout, raw_rx.recv())
            .await
            .map_err(|_| "ZMODEM receive timeout (no data from remote)".to_string())?;

        let Some(chunk) = chunk_opt else {
            return Err("Terminal stream closed during ZMODEM download".to_string());
        };

        stream_buffer.extend_from_slice(&chunk);

        // Check for cancel sequence: 5x CAN (0x18)
        if stream_buffer.windows(5).any(|w| w == [ZDLE, ZDLE, ZDLE, ZDLE, ZDLE]) {
            log::info!("[ZMODEM-SESSION] Remote cancelled transfer, draining in-flight bytes...");
            abort_and_drain_stream(&terminal, &mut raw_rx).await;
            return Err("ZMODEM transfer was cancelled by remote".to_string());
        }

        // Process any headers in buffer
        while let Some((hdr, _start, end)) = parse_any_header(&stream_buffer) {
            // Drain up to the header
            stream_buffer.drain(..end);

            match hdr.parsed_type() {
                ZmodemHeaderType::Zfile => {
                    file_idx += 1;
                    // Next comes the ZFILE payload subpacket
                    // Wait for subpacket to decode filename and size
                    let mut payload_opt = None;
                    for _ in 0..10 {
                        if let Some((payload, _end_delim, consumed)) = decode_subpacket(&stream_buffer, false)
                            .or_else(|| decode_subpacket(&stream_buffer, true))
                        {
                            stream_buffer.drain(..consumed);
                            payload_opt = Some(payload);
                            break;
                        }
                        if let Ok(Some(next_chunk)) = tokio::time::timeout(std::time::Duration::from_millis(200), raw_rx.recv()).await {
                            stream_buffer.extend_from_slice(&next_chunk);
                        } else {
                            break;
                        }
                    }

                    let payload = payload_opt.unwrap_or_default();
                    let (remote_name, file_size) = parse_zfile_payload(&payload)
                        .unwrap_or_else(|| (format!("download_{}", file_idx), 0));

                    let local_path = resolve_unique_download_path(&target_dir, &remote_name);
                    let file = std::fs::File::create(&local_path)
                        .map_err(|e| format!("Failed to create local file {:?}: {}", local_path, e))?;

                    current_file.0 = Some((file, local_path.clone(), file_size, 0, Instant::now()));
                    last_saved_path = local_path;

                    // Reply ZRPOS(0) to request data from 0
                    let zrpos_hdr = build_hex_header(ZRPOS, [0, 0, 0, 0]);
                    terminal.send_bytes(&zrpos_hdr);
                }
                ZmodemHeaderType::Zdata(_pos) => {
                    // Data header received, data subpackets follow in stream_buffer
                }
                ZmodemHeaderType::Zeof => {
                    // Flush current file
                    if let Some((mut f, _, _, _, _)) = current_file.0.take() {
                        let _ = f.flush();
                    }
                    // Reply ZRINIT to acknowledge EOF
                    let zrinit_hdr = build_hex_header(ZRINIT, [0, 0, 0, CANFDX | CANOVIO | CANFC32]);
                    terminal.send_bytes(&zrinit_hdr);
                }
                ZmodemHeaderType::Zfin => {
                    // End of session
                    if let Some((mut f, _, _, _, _)) = current_file.0.take() {
                        let _ = f.flush();
                    }
                    let zfin_hdr = build_hex_header(ZFIN, [0, 0, 0, 0]);
                    terminal.send_bytes(&zfin_hdr);
                    terminal.send_bytes(b"OO\r");
                    log::info!("[ZMODEM-SESSION] Download completed successfully | path={:?}", last_saved_path);
                    return Ok(last_saved_path);
                }
                ZmodemHeaderType::Zcan => {
                    log::info!("[ZMODEM-SESSION] Received Zcan from remote, draining...");
                    abort_and_drain_stream(&terminal, &mut raw_rx).await;
                    return Err("Transfer cancelled by remote".to_string());
                }
                ZmodemHeaderType::Zrqinit => {
                    let zrinit_hdr = build_hex_header(ZRINIT, [0, 0, 0, CANFDX | CANOVIO | CANFC32]);
                    terminal.send_bytes(&zrinit_hdr);
                }
                _ => {}
            }
        }

        // If file is open, decode and write data subpackets
        if let Some((ref mut file, ref path, total, ref mut written, start_time)) = current_file.0 {
            let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();

            // Try decoding subpackets (CRC32 first, fallback to CRC16)
            while let Some((sub_payload, frame_end, consumed)) = decode_subpacket(&stream_buffer, true)
                .or_else(|| decode_subpacket(&stream_buffer, false))
            {
                if cancel_flag.load(Ordering::Relaxed) {
                    log::info!("[ZMODEM-SESSION] Download cancelled by user during decoding, draining...");
                    abort_and_drain_stream(&terminal, &mut raw_rx).await;
                    return Err("Download cancelled by user".to_string());
                }
                stream_buffer.drain(..consumed);
                if !sub_payload.is_empty() {
                    file.write_all(&sub_payload)
                        .map_err(|e| format!("Failed to write to file: {}", e))?;
                    *written += sub_payload.len() as u64;
                    let elapsed = start_time.elapsed().as_secs_f64().max(0.001);
                    let speed = *written as f64 / elapsed;
                    progress_cb(*written, total, speed, &filename, file_idx, 1);
                }

                // If sender requested ACK (ZCRCQ or ZCRCW), reply ZACK(written)
                if frame_end == ZCRCQ || frame_end == ZCRCW {
                    let zack_hdr = build_hex_header(ZACK, (*written as u32).to_le_bytes());
                    terminal.send_bytes(&zack_hdr);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zfile_payload_roundtrip() {
        let payload = build_zfile_payload("test_document.pdf", 987654);
        let (name, size) = parse_zfile_payload(&payload).expect("parsed zfile payload");
        assert_eq!(name, "test_document.pdf");
        assert_eq!(size, 987654);
    }

    #[test]
    fn test_resolve_unique_download_path() {
        let tmp = std::env::temp_dir();
        let test_file = format!("velowork_uniq_{}.txt", uuid::Uuid::new_v4());
        let path1 = resolve_unique_download_path(&tmp, &test_file);
        assert_eq!(path1, tmp.join(&test_file));

        std::fs::write(&path1, b"sample").unwrap();
        let path2 = resolve_unique_download_path(&tmp, &test_file);
        assert!(path2.to_string_lossy().contains("(1)"));
        let _ = std::fs::remove_file(path1);
    }

    #[test]
    fn test_zmodem_skip_reason_labels_and_descriptions() {
        let protected = ZmodemSkipReason::Protected;
        let file_exists = ZmodemSkipReason::FileExists;

        assert_eq!(protected.label(), "受保护 (Protected)");
        assert!(protected.description().contains("服务端目标文件受保护或无写入权限"));
        assert!(protected.description().contains("Protected: Permission denied"));

        assert_eq!(file_exists.label(), "已跳过 (Skipped)");
        assert!(file_exists.description().contains("服务端已存在同名文件"));
        assert!(file_exists.description().contains("Skipped: File already exists"));
    }

    #[test]
    fn test_zmodem_upload_summary_default() {
        let summary = ZmodemUploadSummary::default();
        assert_eq!(summary.total_files, 0);
        assert_eq!(summary.transferred_files, 0);
        assert_eq!(summary.transferred_bytes, 0);
        assert!(summary.skipped_files.is_empty());
    }

    #[test]
    fn test_zmodem_upload_event_variants() {
        let prog_event = ZmodemUploadEvent::Progress {
            transferred: 100,
            total: 200,
            speed: 50.0,
            filename: "foo.txt".to_string(),
            file_idx: 1,
            total_files: 1,
        };
        let skip_event = ZmodemUploadEvent::FileSkipped {
            filename: "bar.txt".to_string(),
            reason: ZmodemSkipReason::Protected,
        };
        assert_ne!(prog_event, skip_event);
    }
}
