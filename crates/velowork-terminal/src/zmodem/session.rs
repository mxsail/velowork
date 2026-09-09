//! ZMODEM file transfer session helper routines for upload (rz) and download (sz).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::Receiver;

use super::protocol::*;

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

/// Encode a ZMODEM data subpacket (end frame ZCRCW = 'k' or 0x6b).
pub fn encode_zdata_frame(data: &[u8], is_end: bool) -> Vec<u8> {
    let frame_end = if is_end { ZCRCE } else { ZCRCW };
    let encoded_data = zdle_encode(data);

    let mut raw_crc_bytes = Vec::with_capacity(data.len() + 1);
    raw_crc_bytes.extend_from_slice(data);
    raw_crc_bytes.push(frame_end);
    let crc = crc16(&raw_crc_bytes);

    let mut out = Vec::with_capacity(encoded_data.len() + 10);
    out.extend_from_slice(&encoded_data);
    out.push(ZDLE);
    out.push(frame_end);

    let crc_bytes = crc.to_be_bytes();
    out.extend_from_slice(&zdle_encode(&crc_bytes));
    out
}

/// Send a local file over ZMODEM protocol with progress updates.
pub async fn send_zmodem_upload_with_progress<F>(
    terminal: Arc<crate::terminal::Terminal>,
    local_path: PathBuf,
    mut progress_cb: F,
) -> Result<(), String>
where
    F: FnMut(u64, u64, f64),
{
    let filename = local_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let bytes = std::fs::read(&local_path).map_err(|e| format!("Read file error: {:?}", e))?;
    let file_size = bytes.len() as u64;

    // 1. Send ZFILE header
    let zfile_hdr = build_hex_header(ZFILE, [0, 0, 0, 0]);
    terminal.send_bytes(&zfile_hdr);

    // 2. Send ZFILE payload
    let zfile_payload = build_zfile_payload(&filename, file_size);
    let zfile_subpacket = encode_zdata_frame(&zfile_payload, false);
    terminal.send_bytes(&zfile_subpacket);

    tokio::time::sleep(std::time::Duration::from_millis(60)).await;

    // 3. Send ZDATA header
    let zdata_hdr = build_hex_header(ZDATA, [0, 0, 0, 0]);
    terminal.send_bytes(&zdata_hdr);

    // 4. Send content in chunks
    let start = Instant::now();
    let chunk_size = 8192;
    let mut pos = 0usize;
    while pos < bytes.len() {
        let end = (pos + chunk_size).min(bytes.len());
        let is_last = end >= bytes.len();
        let chunk = &bytes[pos..end];
        let frame = encode_zdata_frame(chunk, is_last);
        terminal.send_bytes(&frame);
        pos = end;
        let transferred = pos as u64;
        let elapsed = start.elapsed().as_secs_f64().max(0.001);
        let speed_bps = transferred as f64 / elapsed;
        progress_cb(transferred, file_size, speed_bps);
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
    }

    // 5. Send ZEOF header
    let zeof_hdr = build_hex_header(ZEOF, (file_size as u32).to_le_bytes());
    terminal.send_bytes(&zeof_hdr);
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;

    // 6. Send ZFIN header
    let zfin_hdr = build_hex_header(ZFIN, [0, 0, 0, 0]);
    terminal.send_bytes(&zfin_hdr);

    // 7. Send "OO" to complete session
    terminal.send_bytes(b"OO");
    Ok(())
}

/// Receive a file downloaded from remote via ZMODEM (`sz`) protocol.
pub async fn receive_zmodem_download_with_progress<F>(
    terminal: Arc<crate::terminal::Terminal>,
    target_dir: PathBuf,
    mut raw_rx: Receiver<Vec<u8>>,
    mut progress_cb: F,
) -> Result<PathBuf, String>
where
    F: FnMut(u64, u64, f64, &str),
{
    use std::io::Write;

    let start_time = Instant::now();

    // 1. Send ZRINIT to indicate readiness
    let zrinit_hdr = build_hex_header(ZRINIT, [CANFDX | CANOVIO | CANFC32, 0, 0, 0]);
    terminal.send_bytes(&zrinit_hdr);

    let mut stream_buffer = Vec::with_capacity(65536);
    let mut file_handle: Option<(std::fs::File, PathBuf, u64, u64)> = None; // (file, path, total_size, written)

    let timeout = std::time::Duration::from_secs(120);

    loop {
        let chunk_opt = tokio::time::timeout(timeout, raw_rx.recv())
            .await
            .map_err(|_| "ZMODEM receive timeout (no data from remote)".to_string())?;

        let Some(chunk) = chunk_opt else {
            return Err("PTY stream closed during ZMODEM download".to_string());
        };

        stream_buffer.extend_from_slice(&chunk);

        // Check for cancel sequence: 5x CAN (0x18)
        if stream_buffer.windows(5).any(|w| w == [ZDLE, ZDLE, ZDLE, ZDLE, ZDLE]) {
            return Err("ZMODEM transfer was canceled by remote".to_string());
        }

        // Process hex headers in the stream
        while let Some((htype, flags, consumed)) = parse_hex_header(&stream_buffer) {
            let header = ZmodemHeaderType::from_u8(htype, flags);
            let mut remaining_payload = stream_buffer[consumed..].to_vec();
            stream_buffer.clear();

            match header {
                ZmodemHeaderType::Zfile => {
                    // Extract ZFILE payload (unescaped)
                    let (decoded, payload_consumed) = zdle_decode(&remaining_payload);
                    if let Some((remote_name, file_size)) = parse_zfile_payload(&decoded) {
                        let final_path = resolve_unique_download_path(&target_dir, &remote_name);
                        let file = std::fs::File::create(&final_path)
                            .map_err(|e| format!("Failed to create local file {:?}: {}", final_path, e))?;
                        file_handle = Some((file, final_path.clone(), file_size, 0));

                        // Reply ZRPOS(0) to request data from beginning
                        let zrpos_hdr = build_hex_header(ZRPOS, [0, 0, 0, 0]);
                        terminal.send_bytes(&zrpos_hdr);
                    }
                    if payload_consumed < remaining_payload.len() {
                        remaining_payload.drain(..payload_consumed);
                        stream_buffer = remaining_payload;
                    }
                }
                ZmodemHeaderType::Zdata(pos) => {
                    // Data header; remaining payload contains data subpackets
                    if let Some((ref mut file, ref path, total, ref mut written)) = file_handle {
                        if pos as u64 != *written {
                            // Sync position if needed
                        }
                        let (decoded_data, _) = zdle_decode(&remaining_payload);
                        if !decoded_data.is_empty() {
                            file.write_all(&decoded_data)
                                .map_err(|e| format!("Write error: {}", e))?;
                            *written += decoded_data.len() as u64;
                            let elapsed = start_time.elapsed().as_secs_f64().max(0.001);
                            let speed_bps = *written as f64 / elapsed;
                            progress_cb(*written, total, speed_bps, &path.to_string_lossy());
                        }
                    }
                }
                ZmodemHeaderType::Zeof => {
                    // EOF reached, flush file and send ZRINIT to acknowledge
                    if let Some((mut file, _, _, _)) = file_handle.take() {
                        let _ = file.flush();
                    }
                    let zrinit_hdr = build_hex_header(ZRINIT, [CANFDX | CANOVIO | CANFC32, 0, 0, 0]);
                    terminal.send_bytes(&zrinit_hdr);
                }
                ZmodemHeaderType::Zfin => {
                    // End of session: send ZFIN and "OO"
                    let zfin_hdr = build_hex_header(ZFIN, [0, 0, 0, 0]);
                    terminal.send_bytes(&zfin_hdr);
                    terminal.send_bytes(b"OO");

                    if let Some((_, path, _, _)) = file_handle {
                        return Ok(path);
                    }
                    return Ok(target_dir.join("downloaded_file"));
                }
                ZmodemHeaderType::Zcan => {
                    return Err("ZMODEM transfer was canceled by remote".to_string());
                }
                _ => {}
            }
        }

        // If file is open and we have raw stream data without standard header, write unescaped data
        if let Some((ref mut file, ref path, total, ref mut written)) = file_handle {
            if stream_buffer.len() > 1024 && !stream_buffer.windows(4).any(|w| w == [ZPAD, ZPAD, ZDLE, ZHEX]) {
                let (decoded, consumed) = zdle_decode(&stream_buffer);
                if !decoded.is_empty() {
                    file.write_all(&decoded)
                        .map_err(|e| format!("Write error: {}", e))?;
                    *written += decoded.len() as u64;
                    let elapsed = start_time.elapsed().as_secs_f64().max(0.001);
                    let speed_bps = *written as f64 / elapsed;
                    progress_cb(*written, total, speed_bps, &path.to_string_lossy());
                }
                stream_buffer.drain(..consumed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zfile_payload_roundtrip() {
        let payload = build_zfile_payload("example.txt", 10240);
        let (filename, size) = parse_zfile_payload(&payload).expect("parsed zfile");
        assert_eq!(filename, "example.txt");
        assert_eq!(size, 10240);
    }

    #[test]
    fn test_resolve_unique_download_path() {
        let temp_dir = std::env::temp_dir();
        let test_name = format!("velowork_test_unique_{}.txt", uuid::Uuid::new_v4());
        let file_path = temp_dir.join(&test_name);

        // Doesn't exist yet
        let resolved = resolve_unique_download_path(&temp_dir, &test_name);
        assert_eq!(resolved, file_path);

        // Create the file
        std::fs::write(&file_path, b"test").expect("write test file");

        // Should resolve to `(1)`
        let resolved2 = resolve_unique_download_path(&temp_dir, &test_name);
        assert!(resolved2.to_string_lossy().contains("(1)"));

        let _ = std::fs::remove_file(file_path);
    }
}
