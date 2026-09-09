//! Cross-platform SSH Agent client implementation.
//!
//! Supports:
//! - Unix Domain Sockets (`$SSH_AUTH_SOCK` on Linux/macOS, or custom socket path)
//! - Windows OpenSSH Named Pipe (`\\.\pipe\openssh-ssh-agent`)
//! - Windows PuTTY / Pageant IPC protocol (`WM_COPYDATA` message + shared memory)

use super::protocol::{
    decode_identities_answer, decode_sign_response, encode_request_identities, encode_sign_request,
    AgentKey, AgentProtocolError,
};
use std::env;
use std::fmt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const DEFAULT_WIN_OPENSSH_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";

#[derive(Debug)]
pub enum AgentClientError {
    NoAgentRunning,
    SocketNotFound(String),
    ConnectionFailed(String),
    Io(std::io::Error),
    Protocol(AgentProtocolError),
    PageantError(String),
}

impl fmt::Display for AgentClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAgentRunning => write!(
                f,
                "No SSH Agent detected (environment variable $SSH_AUTH_SOCK is unset and no standard agent is running)"
            ),
            Self::SocketNotFound(path) => write!(f, "SSH Agent socket/pipe not found: {}", path),
            Self::ConnectionFailed(msg) => write!(f, "Failed to connect to SSH Agent: {}", msg),
            Self::Io(err) => write!(f, "SSH Agent I/O error: {}", err),
            Self::Protocol(err) => write!(f, "SSH Agent protocol error: {}", err),
            Self::PageantError(msg) => write!(f, "Pageant IPC error: {}", msg),
        }
    }
}

impl std::error::Error for AgentClientError {}

impl From<std::io::Error> for AgentClientError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<AgentProtocolError> for AgentClientError {
    fn from(err: AgentProtocolError) -> Self {
        Self::Protocol(err)
    }
}

impl From<russh::SendError> for AgentClientError {
    fn from(_: russh::SendError) -> Self {
        Self::ConnectionFailed("russh channel send error".to_string())
    }
}

/// Underlying transport stream for communicating with the agent.
pub enum AgentStream {
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
    #[cfg(windows)]
    Unix(tokio::net::UnixStream),
    #[cfg(windows)]
    NamedPipe(tokio::net::windows::named_pipe::NamedPipeClient),
    #[cfg(windows)]
    Pageant,
}

impl AgentStream {
    /// Read exactly one framed agent message (length prefix + payload).
    pub async fn read_framed_message(&mut self) -> Result<Vec<u8>, AgentClientError> {
        match self {
            #[cfg(unix)]
            Self::Unix(s) => read_stream_framed(s).await,
            #[cfg(windows)]
            Self::Unix(s) => read_stream_framed(s).await,
            #[cfg(windows)]
            Self::NamedPipe(p) => read_stream_framed(p).await,
            #[cfg(windows)]
            Self::Pageant => Err(AgentClientError::PageantError(
                "Streaming read is not supported on Pageant message IPC".to_string(),
            )),
        }
    }

    /// Write a framed agent message and flush.
    pub async fn write_framed_message(&mut self, framed: &[u8]) -> Result<(), AgentClientError> {
        match self {
            #[cfg(unix)]
            Self::Unix(s) => {
                s.write_all(framed).await?;
                s.flush().await?;
                Ok(())
            }
            #[cfg(windows)]
            Self::Unix(s) => {
                s.write_all(framed).await?;
                s.flush().await?;
                Ok(())
            }
            #[cfg(windows)]
            Self::NamedPipe(p) => {
                p.write_all(framed).await?;
                p.flush().await?;
                Ok(())
            }
            #[cfg(windows)]
            Self::Pageant => Err(AgentClientError::PageantError(
                "Streaming write is not supported on Pageant message IPC".to_string(),
            )),
        }
    }

    /// Send a request frame and await the full framed response.
    pub async fn exchange_raw(&mut self, request_frame: &[u8]) -> Result<Vec<u8>, AgentClientError> {
        match self {
            #[cfg(unix)]
            Self::Unix(s) => {
                s.write_all(request_frame).await?;
                s.flush().await?;
                read_stream_framed(s).await
            }
            #[cfg(windows)]
            Self::Unix(s) => {
                s.write_all(request_frame).await?;
                s.flush().await?;
                read_stream_framed(s).await
            }
            #[cfg(windows)]
            Self::NamedPipe(p) => {
                p.write_all(request_frame).await?;
                p.flush().await?;
                read_stream_framed(p).await
            }
            #[cfg(windows)]
            Self::Pageant => {
                tokio::task::spawn_blocking({
                    let req = request_frame.to_vec();
                    move || pageant_exchange(&req)
                })
                .await
                .map_err(|e| AgentClientError::ConnectionFailed(e.to_string()))?
            }
        }
    }
}

async fn read_stream_framed<R: AsyncReadExt + Unpin>(stream: &mut R) -> Result<Vec<u8>, AgentClientError> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 256 * 1024 {
        return Err(AgentClientError::Protocol(AgentProtocolError::InvalidEncoding(
            format!("Agent message length too large: {} bytes", len),
        )));
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;
    Ok(body)
}

/// Connected SSH Agent client.
pub struct AgentClient {
    stream: AgentStream,
}

impl AgentClient {
    /// Connect to SSH Agent using explicit path or auto-detect system default.
    pub async fn connect(explicit_path: Option<&str>) -> Result<Self, AgentClientError> {
        let stream = Self::connect_stream(explicit_path).await?;
        Ok(Self { stream })
    }

    /// Open an `AgentStream` connection.
    pub async fn connect_stream(explicit_path: Option<&str>) -> Result<AgentStream, AgentClientError> {
        if let Some(path) = explicit_path {
            let p = path.trim();
            if !p.is_empty() {
                return Self::connect_explicit_path(p).await;
            }
        }

        Self::connect_default().await
    }

    async fn connect_explicit_path(path: &str) -> Result<AgentStream, AgentClientError> {
        #[cfg(windows)]
        {
            if path.starts_with(r"\\.\pipe\") {
                match tokio::net::windows::named_pipe::ClientOptions::new().open(path) {
                    Ok(client) => return Ok(AgentStream::NamedPipe(client)),
                    Err(err) => return Err(AgentClientError::SocketNotFound(format!("{}: {}", path, err))),
                }
            }
            if path.eq_ignore_ascii_case("pageant") {
                if is_pageant_available() {
                    return Ok(AgentStream::Pageant);
                } else {
                    return Err(AgentClientError::SocketNotFound("Pageant window not found".to_string()));
                }
            }
        }

        // Try Unix domain socket
        match tokio::net::UnixStream::connect(path).await {
            Ok(stream) => Ok(AgentStream::Unix(stream)),
            Err(err) => Err(AgentClientError::SocketNotFound(format!("{}: {}", path, err))),
        }
    }

    async fn connect_default() -> Result<AgentStream, AgentClientError> {
        // 1. Check $SSH_AUTH_SOCK environment variable
        if let Ok(sock) = env::var("SSH_AUTH_SOCK") {
            let sock_trim = sock.trim();
            if !sock_trim.is_empty() {
                if let Ok(stream) = Self::connect_explicit_path(sock_trim).await {
                    return Ok(stream);
                }
            }
        }

        #[cfg(windows)]
        {
            // 2. Try Windows OpenSSH named pipe
            if let Ok(client) = tokio::net::windows::named_pipe::ClientOptions::new().open(DEFAULT_WIN_OPENSSH_PIPE) {
                return Ok(AgentStream::NamedPipe(client));
            }

            // 3. Try PuTTY / Pageant IPC
            if is_pageant_available() {
                return Ok(AgentStream::Pageant);
            }
        }

        Err(AgentClientError::NoAgentRunning)
    }

    /// Query all public key identities stored in the Agent.
    pub async fn request_identities(&mut self) -> Result<Vec<AgentKey>, AgentClientError> {
        let req = encode_request_identities();
        let resp = self.stream.exchange_raw(&req).await?;
        let keys = decode_identities_answer(&resp)?;
        Ok(keys)
    }

    /// Request a cryptographic signature from the Agent for a public key blob.
    pub async fn sign_request(
        &mut self,
        key_blob: &[u8],
        data_to_sign: &[u8],
        flags: u32,
    ) -> Result<Vec<u8>, AgentClientError> {
        let req = encode_sign_request(key_blob, data_to_sign, flags);
        let resp = self.stream.exchange_raw(&req).await?;
        let sig = decode_sign_response(&resp)?;
        Ok(sig)
    }

    /// Exchange a raw agent request frame (used for Agent Forwarding proxying).
    pub async fn exchange_raw(&mut self, request_frame: &[u8]) -> Result<Vec<u8>, AgentClientError> {
        self.stream.exchange_raw(request_frame).await
    }

    /// Access mutable reference to underlying stream.
    pub fn stream_mut(&mut self) -> &mut AgentStream {
        &mut self.stream
    }
}

// ---------------------------------------------------------------------------
// Windows Pageant IPC Implementation
// ---------------------------------------------------------------------------

#[cfg(windows)]
const AGENT_COPYDATA_ID: usize = 0x804e50ba;
#[cfg(windows)]
const AGENT_MAX_MSGLEN: usize = 8192;

#[cfg(windows)]
fn is_pageant_available() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowA;
    unsafe {
        let class_name = b"Pageant\0";
        let hwnd = FindWindowA(class_name.as_ptr(), class_name.as_ptr());
        hwnd != 0
    }
}

#[cfg(not(windows))]
#[allow(dead_code)]
fn is_pageant_available() -> bool {
    false
}

#[cfg(windows)]
fn pageant_exchange(request_frame: &[u8]) -> Result<Vec<u8>, AgentClientError> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Memory::{
        CreateFileMappingA, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowA, SendMessageA, COPYDATASTRUCT, WM_COPYDATA,
    };

    if request_frame.len() > AGENT_MAX_MSGLEN {
        return Err(AgentClientError::PageantError("Request exceeds Pageant maximum message length".to_string()));
    }

    unsafe {
        let class_name = b"Pageant\0";
        let hwnd = FindWindowA(class_name.as_ptr(), class_name.as_ptr());
        if hwnd == 0 {
            return Err(AgentClientError::PageantError("Pageant window not found".to_string()));
        }

        let map_name_str = format!("PageantRequest_{}_{}\0", std::process::id(), fastrand::u64(..));
        let map_name = map_name_str.as_bytes();

        let h_map = CreateFileMappingA(
            INVALID_HANDLE_VALUE,
            null_mut(),
            PAGE_READWRITE,
            0,
            AGENT_MAX_MSGLEN as u32,
            map_name.as_ptr(),
        );
        if h_map == 0 {
            return Err(AgentClientError::PageantError(format!(
                "Failed to create file mapping: error {}",
                GetLastError()
            )));
        }

        let p_map = MapViewOfFile(h_map, FILE_MAP_ALL_ACCESS, 0, 0, AGENT_MAX_MSGLEN);
        if p_map.is_null() {
            CloseHandle(h_map);
            return Err(AgentClientError::PageantError(format!(
                "Failed to map view of file: error {}",
                GetLastError()
            )));
        }

        // Copy request into mapped memory
        std::ptr::copy_nonoverlapping(request_frame.as_ptr(), p_map as *mut u8, request_frame.len());

        let cds = COPYDATASTRUCT {
            dwData: AGENT_COPYDATA_ID,
            cbData: map_name.len() as u32,
            lpData: map_name.as_ptr() as *mut _,
        };

        let res = SendMessageA(hwnd, WM_COPYDATA, 0, &cds as *const _ as isize);
        if res == 0 {
            UnmapViewOfFile(p_map);
            CloseHandle(h_map);
            return Err(AgentClientError::PageantError("Pageant WM_COPYDATA message failed".to_string()));
        }

        // Read response: first 4 bytes are length
        let mut len_bytes = [0u8; 4];
        std::ptr::copy_nonoverlapping(p_map as *const u8, len_bytes.as_mut_ptr(), 4);
        let resp_len = u32::from_be_bytes(len_bytes) as usize;

        if resp_len + 4 > AGENT_MAX_MSGLEN {
            UnmapViewOfFile(p_map);
            CloseHandle(h_map);
            return Err(AgentClientError::PageantError("Invalid Pageant response length".to_string()));
        }

        let mut body = vec![0u8; resp_len];
        std::ptr::copy_nonoverlapping(
            (p_map as *const u8).add(4),
            body.as_mut_ptr(),
            resp_len,
        );

        UnmapViewOfFile(p_map);
        CloseHandle(h_map);

        Ok(body)
    }
}
