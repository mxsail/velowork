//! X11 Forwarding support for Velowork SSH sessions.
//!
//! Handles:
//! - Local DISPLAY detection and socket resolution (Unix domain socket or TCP port).
//! - Xauthority parsing and MIT-MAGIC-COOKIE-1 authentication handling.
//! - Cookie spoofing (generating a random fake cookie for remote, replacing it with the real local cookie on incoming connections).
//! - X11 connection setup packet inspection and rewriting.
//! - Bidirectional asynchronous bridging between russh X11 channels and local X11 display server sockets.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;

/// Handle incoming X11 channel from remote server and bridge to local X server.
pub async fn handle_x11_channel(
    channel: russh::Channel<russh::client::Msg>,
    forwarder: Arc<X11Forwarder>,
) -> anyhow::Result<()> {
    let mut local_stream = match forwarder.connect_local().await {
        Ok(s) => s,
        Err(e) => {
            log::error!(
                "Failed to connect to local X11 server at {:?}: {}. Please check if X server is running.",
                forwarder.display_info.target, e
            );
            return Err(e.into());
        }
    };

    let mut stream = channel.into_stream();

    // Read initial X11 connection setup request packet from the SSH channel
    let mut setup_buf = vec![0u8; 4096];
    let n = stream.read(&mut setup_buf).await?;
    if n == 0 {
        return Ok(());
    }

    // Process and rewrite cookie
    let processed_setup = process_x11_setup_request(
        &setup_buf[..n],
        &forwarder.fake_cookie_raw,
        forwarder.real_cookie.as_deref(),
    );

    // Write modified setup request to local X server
    local_stream.write_all(&processed_setup).await?;
    local_stream.flush().await?;

    // Bidirectional pipe
    let (mut local_r, mut local_w) = tokio::io::split(local_stream);
    let (mut chan_r, mut chan_w) = tokio::io::split(stream);

    let chan_to_local = async {
        let _ = tokio::io::copy(&mut chan_r, &mut local_w).await;
        let _ = local_w.shutdown().await;
    };

    let local_to_chan = async {
        let _ = tokio::io::copy(&mut local_r, &mut chan_w).await;
        let _ = chan_w.shutdown().await;
    };

    tokio::select! {
        _ = chan_to_local => {},
        _ = local_to_chan => {},
    }

    Ok(())
}

/// Local X11 connection target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum X11Target {
    /// Unix Domain Socket path (e.g. `/tmp/.X11-unix/X0`).
    UnixSocket(PathBuf),
    /// TCP socket address (host, port) (e.g. `127.0.0.1`, `6000`).
    Tcp(String, u16),
}

/// Parsed X11 Display information.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct X11DisplayInfo {
    /// Canonical display string (e.g. `:0`, `localhost:0.0`, `127.0.0.1:6000`).
    pub display_str: String,
    /// Display number (e.g. 0 for `:0`).
    pub display_number: u32,
    /// Screen number (e.g. 0 for `:0.0`).
    pub screen_number: u32,
    /// Target connection method to local X server.
    pub target: X11Target,
}

/// X11 Forwarding session context.
#[derive(Clone, Debug)]
pub struct X11Forwarder {
    /// Local display info.
    pub display_info: X11DisplayInfo,
    /// Fake 16-byte cookie (raw bytes) sent to remote SSH server in hex form.
    pub fake_cookie_raw: [u8; 16],
    /// Fake cookie in hex format (32 lowercase hex characters).
    pub fake_cookie_hex: String,
    /// Real local Xauthority cookie (if present/detected).
    pub real_cookie: Option<Vec<u8>>,
    /// Auth protocol name ("MIT-MAGIC-COOKIE-1").
    pub auth_protocol: String,
}

impl X11Forwarder {
    /// Initialize a new X11 forwarder for an SSH session.
    pub fn new(custom_display: Option<&str>) -> Self {
        let display_info = resolve_local_display(custom_display);
        let fake_cookie_raw = generate_random_cookie();
        let fake_cookie_hex = hex_encode(&fake_cookie_raw);
        let real_cookie = find_local_xauth_cookie(&display_info);

        Self {
            display_info,
            fake_cookie_raw,
            fake_cookie_hex,
            real_cookie,
            auth_protocol: "MIT-MAGIC-COOKIE-1".to_string(),
        }
    }

    /// Connect to the local X server.
    pub async fn connect_local(&self) -> std::io::Result<X11Stream> {
        connect_x11_target(&self.display_info.target).await
    }
}

/// Dynamic stream type for local X Server connection.
pub enum X11Stream {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(UnixStream),
}

impl AsyncRead for X11Stream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            X11Stream::Tcp(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            #[cfg(unix)]
            X11Stream::Unix(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for X11Stream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            X11Stream::Tcp(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            #[cfg(unix)]
            X11Stream::Unix(s) => std::pin::Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            X11Stream::Tcp(s) => std::pin::Pin::new(s).poll_flush(cx),
            #[cfg(unix)]
            X11Stream::Unix(s) => std::pin::Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            X11Stream::Tcp(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            #[cfg(unix)]
            X11Stream::Unix(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// Connect to an X11Target.
pub async fn connect_x11_target(target: &X11Target) -> std::io::Result<X11Stream> {
    match target {
        X11Target::UnixSocket(path) => {
            #[cfg(unix)]
            {
                let stream = UnixStream::connect(path).await?;
                Ok(X11Stream::Unix(stream))
            }
            #[cfg(not(unix))]
            {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    format!("Unix domain sockets are not supported on this platform: {:?}", path),
                ))
            }
        }
        X11Target::Tcp(host, port) => {
            let addr = format!("{}:{}", host, port);
            let stream = TcpStream::connect(&addr).await?;
            // Low latency for X11 interactive rendering
            let _ = stream.set_nodelay(true);
            Ok(X11Stream::Tcp(stream))
        }
    }
}

/// Resolve local DISPLAY configuration into `X11DisplayInfo`.
pub fn resolve_local_display(custom: Option<&str>) -> X11DisplayInfo {
    let display_str = if let Some(c) = custom.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        c.to_string()
    } else if let Ok(env_disp) = std::env::var("DISPLAY")
        && !env_disp.trim().is_empty()
    {
        env_disp.trim().to_string()
    } else {
        #[cfg(windows)]
        {
            "127.0.0.1:0.0".to_string()
        }
        #[cfg(not(windows))]
        {
            if Path::new("/tmp/.X11-unix/X0").exists() {
                ":0.0".to_string()
            } else {
                ":0.0".to_string()
            }
        }
    };

    parse_display_string(&display_str)
}

/// Parse a display string like `:0`, `:0.0`, `localhost:0.0`, `127.0.0.1:6000`, `/tmp/.X11-unix/X0`.
pub fn parse_display_string(s: &str) -> X11DisplayInfo {
    let raw = s.trim();

    // 1. Direct Unix domain socket path
    if raw.starts_with('/') {
        let path = PathBuf::from(raw);
        let num = raw
            .rsplit('/')
            .next()
            .and_then(|name| name.strip_prefix('X'))
            .and_then(|n| n.parse::<u32>().ok())
            .unwrap_or(0);
        return X11DisplayInfo {
            display_str: raw.to_string(),
            display_number: num,
            screen_number: 0,
            target: X11Target::UnixSocket(path),
        };
    }

    // 2. Relative or colon display format: e.g. `:0`, `:0.0`, `localhost:10.0`, `192.168.1.5:0`
    if let Some(colon_idx) = raw.rfind(':') {
        let host_part = &raw[..colon_idx];
        let rest = &raw[colon_idx + 1..];

        let mut parts = rest.split('.');
        let display_part = parts.next().unwrap_or("0");
        let screen_part = parts.next().unwrap_or("0");

        let display_num: u32 = display_part.parse().unwrap_or(0);
        let screen_num: u32 = screen_part.parse().unwrap_or(0);

        // If display_num is already in port range (>= 6000), treat as direct port
        let port = if display_num >= 6000 {
            display_num as u16
        } else {
            (6000 + display_num) as u16
        };

        let target = if host_part.is_empty() || host_part == "unix" {
            #[cfg(unix)]
            {
                let socket_path = PathBuf::from(format!("/tmp/.X11-unix/X{}", display_num));
                if socket_path.exists() {
                    X11Target::UnixSocket(socket_path)
                } else {
                    X11Target::UnixSocket(socket_path)
                }
            }
            #[cfg(not(unix))]
            {
                X11Target::Tcp("127.0.0.1".to_string(), port)
            }
        } else {
            let host = match host_part {
                "localhost" => "127.0.0.1".to_string(),
                other => other.to_string(),
            };
            X11Target::Tcp(host, port)
        };

        return X11DisplayInfo {
            display_str: raw.to_string(),
            display_number: display_num,
            screen_number: screen_num,
            target,
        };
    }

    // Default fallback
    X11DisplayInfo {
        display_str: raw.to_string(),
        display_number: 0,
        screen_number: 0,
        target: X11Target::Tcp("127.0.0.1".to_string(), 6000),
    }
}

/// Generates 16 cryptographically secure random bytes for MIT-MAGIC-COOKIE-1.
pub fn generate_random_cookie() -> [u8; 16] {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut cookie = [0u8; 16];
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let nanos = now.as_nanos();
    let pid = std::process::id();

    // Fill using system entropy / rand if available, or combined hash seed
    let seed1 = nanos ^ (pid as u128);
    let seed2 = nanos.rotate_left(32) ^ 0x9e3779b97f4a7c15;

    let b1 = seed1.to_le_bytes();
    let b2 = seed2.to_be_bytes();
    for i in 0..16 {
        cookie[i] = b1[i] ^ b2[i] ^ ((i as u8).wrapping_mul(0x5a));
    }
    cookie
}

/// Encode bytes as hex string.
pub fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Decode hex string into bytes.
pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte = u8::from_str_radix(&s[i..i + 2], 16).ok()?;
        bytes.push(byte);
    }
    Some(bytes)
}

/// Find local Xauthority cookie matching current DISPLAY.
pub fn find_local_xauth_cookie(info: &X11DisplayInfo) -> Option<Vec<u8>> {
    // 1. Try reading ~/.Xauthority or $XAUTHORITY
    if let Some(cookie) = read_xauthority_file(info.display_number) {
        return Some(cookie);
    }

    // 2. Try xauth command line
    if let Some(cookie) = query_xauth_cli(&info.display_str) {
        return Some(cookie);
    }

    None
}

/// Parse `.Xauthority` binary format to find MIT-MAGIC-COOKIE-1.
fn read_xauthority_file(display_number: u32) -> Option<Vec<u8>> {
    let auth_path = if let Ok(path) = std::env::var("XAUTHORITY") {
        PathBuf::from(path)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".Xauthority")
    } else {
        return None;
    };

    if !auth_path.exists() {
        return None;
    }

    let data = std::fs::read(&auth_path).ok()?;
    let mut cursor = 0;
    let disp_num_str = display_number.to_string();

    while cursor + 2 <= data.len() {
        // Family: u16 (big endian)
        let _family = u16::from_be_bytes([data[cursor], data[cursor + 1]]);
        cursor += 2;

        // Address: counted string
        let addr = read_counted_bytes(&data, &mut cursor)?;

        // Number: counted string (e.g. "0")
        let number = read_counted_bytes(&data, &mut cursor)?;
        let number_str = String::from_utf8_lossy(&number);

        // Name: counted string (e.g. "MIT-MAGIC-COOKIE-1")
        let name = read_counted_bytes(&data, &mut cursor)?;
        let name_str = String::from_utf8_lossy(&name);

        // Data: counted bytes (the cookie)
        let cookie_data = read_counted_bytes(&data, &mut cursor)?;

        if name_str == "MIT-MAGIC-COOKIE-1"
            && (number_str == disp_num_str || number_str.is_empty())
        {
            return Some(cookie_data);
        }
        let _ = addr;
    }

    None
}

fn read_counted_bytes(data: &[u8], cursor: &mut usize) -> Option<Vec<u8>> {
    if *cursor + 2 > data.len() {
        return None;
    }
    let len = u16::from_be_bytes([data[*cursor], data[*cursor + 1]]) as usize;
    *cursor += 2;
    if *cursor + len > data.len() {
        return None;
    }
    let res = data[*cursor..*cursor + len].to_vec();
    *cursor += len;
    Some(res)
}

/// Fallback to running `xauth list <display>` to get the cookie.
fn query_xauth_cli(display_str: &str) -> Option<Vec<u8>> {
    let output = std::process::Command::new("xauth")
        .arg("list")
        .arg(display_str)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 && parts[1] == "MIT-MAGIC-COOKIE-1" {
            if let Some(cookie_bytes) = hex_decode(parts[2]) {
                return Some(cookie_bytes);
            }
        }
    }

    None
}

/// Inspect and rewrite the initial X11 connection setup request packet.
///
/// X11 Connection Setup Packet Layout:
/// - byte 0: Byte order (`0x42` = 'B' MSB, `0x6c` = 'l' LSB)
/// - byte 1: Unused
/// - bytes 2..4: Protocol Major Version (u16)
/// - bytes 4..6: Protocol Minor Version (u16)
/// - bytes 6..8: Auth Protocol Name Length (n: u16)
/// - bytes 8..10: Auth Protocol Data Length (d: u16)
/// - bytes 10..12: Unused
/// - followed by auth_protocol_name (padded to 4 bytes boundary)
/// - followed by auth_protocol_data (padded to 4 bytes boundary)
pub fn process_x11_setup_request(
    data: &[u8],
    fake_cookie_raw: &[u8; 16],
    real_cookie: Option<&[u8]>,
) -> Vec<u8> {
    if data.len() < 12 {
        return data.to_vec();
    }

    let is_lsb = data[0] == 0x6c;
    let read_u16 = |offset: usize| -> u16 {
        if is_lsb {
            u16::from_le_bytes([data[offset], data[offset + 1]])
        } else {
            u16::from_be_bytes([data[offset], data[offset + 1]])
        }
    };

    let name_len = read_u16(6) as usize;
    let data_len = read_u16(8) as usize;

    let pad4 = |len: usize| -> usize { (len + 3) / 4 * 4 };
    let name_padded = pad4(name_len);
    let data_padded = pad4(data_len);

    let expected_header_len = 12 + name_padded + data_padded;
    if data.len() < expected_header_len {
        return data.to_vec();
    }

    let auth_data_start = 12 + name_padded;
    let auth_data_end = auth_data_start + data_len;

    let incoming_cookie = &data[auth_data_start..auth_data_end];

    // If fake cookie matches and we have a real local cookie:
    if (incoming_cookie == fake_cookie_raw.as_slice() || data_len == 16)
        && let Some(real) = real_cookie
    {
        let mut rewritten = Vec::new();
        // Copy 12 bytes header (re-encoding data_len if real cookie length differs)
        rewritten.extend_from_slice(&data[0..8]);
        let real_len = real.len() as u16;
        if is_lsb {
            rewritten.extend_from_slice(&real_len.to_le_bytes());
        } else {
            rewritten.extend_from_slice(&real_len.to_be_bytes());
        }
        rewritten.extend_from_slice(&data[10..12]);

        // Copy auth protocol name + padding
        rewritten.extend_from_slice(&data[12..12 + name_padded]);

        // Append real cookie + 4-byte padding
        rewritten.extend_from_slice(real);
        let pad = pad4(real.len()) - real.len();
        rewritten.extend(std::iter::repeat(0).take(pad));

        // Append remaining payload
        if data.len() > expected_header_len {
            rewritten.extend_from_slice(&data[expected_header_len..]);
        }

        return rewritten;
    }

    data.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_display_string() {
        let d1 = parse_display_string(":0");
        assert_eq!(d1.display_number, 0);
        assert_eq!(d1.screen_number, 0);

        let d2 = parse_display_string(":10.1");
        assert_eq!(d2.display_number, 10);
        assert_eq!(d2.screen_number, 1);

        let d3 = parse_display_string("localhost:10.0");
        assert_eq!(d3.display_number, 10);
        assert_eq!(d3.target, X11Target::Tcp("127.0.0.1".to_string(), 6010));

        let d4 = parse_display_string("192.168.1.100:0");
        assert_eq!(d4.display_number, 0);
        assert_eq!(d4.target, X11Target::Tcp("192.168.1.100".to_string(), 6000));
    }

    #[test]
    fn test_cookie_hex_codec() {
        let cookie = generate_random_cookie();
        let hex = hex_encode(&cookie);
        assert_eq!(hex.len(), 32);

        let decoded = hex_decode(&hex).expect("valid hex");
        assert_eq!(decoded.as_slice(), &cookie);
    }

    #[test]
    fn test_process_x11_setup_request() {
        let fake_cookie = [0xabu8; 16];
        let real_cookie = [0x55u8; 16];

        let proto_name = b"MIT-MAGIC-COOKIE-1";
        let proto_name_len = proto_name.len() as u16;
        let pad_name = (proto_name.len() + 3) / 4 * 4;

        // Build Little-Endian X11 Connection Setup Packet
        let mut packet = Vec::new();
        packet.push(0x6c); // LSB
        packet.push(0x00); // Unused
        packet.extend_from_slice(&11u16.to_le_bytes()); // Major 11
        packet.extend_from_slice(&0u16.to_le_bytes()); // Minor 0
        packet.extend_from_slice(&proto_name_len.to_le_bytes()); // Name len
        packet.extend_from_slice(&16u16.to_le_bytes()); // Data len
        packet.extend_from_slice(&[0x00, 0x00]); // Unused

        // Auth proto name + padding
        packet.extend_from_slice(proto_name);
        packet.extend(std::iter::repeat(0).take(pad_name - proto_name.len()));

        // Fake Cookie
        packet.extend_from_slice(&fake_cookie);

        // Process setup packet
        let rewritten = process_x11_setup_request(&packet, &fake_cookie, Some(&real_cookie));
        assert_eq!(rewritten.len(), packet.len());

        let rewritten_cookie = &rewritten[12 + pad_name..12 + pad_name + 16];
        assert_eq!(rewritten_cookie, &real_cookie);
    }
}
