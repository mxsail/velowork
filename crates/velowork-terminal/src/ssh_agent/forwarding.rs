//! SSH Agent Forwarding (`auth-agent@openssh.com`) handler and proxy loop.

use super::client::AgentClient;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const AGENT_FORWARDING_CHANNEL_TYPE: &str = "auth-agent@openssh.com";
pub const AGENT_FORWARDING_REQ_TYPE: &str = "auth-agent-req@openssh.com";

/// Handle incoming Agent forwarding channel from the remote server and bridge to the local SSH agent.
pub async fn handle_agent_forwarding_channel(
    channel: russh::Channel<russh::client::Msg>,
    socket_path: Option<String>,
) -> anyhow::Result<()> {
    let mut local_client = match AgentClient::connect(socket_path.as_deref()).await {
        Ok(c) => c,
        Err(err) => {
            log::warn!("[terminal:ssh_agent] Failed to connect to local SSH agent for forwarding | error: {:#}", err);
            return Err(anyhow::anyhow!("Local SSH agent connection failed: {}", err));
        }
    };

    let mut chan_stream = channel.into_stream();
    let mut buf = Vec::new();
    let mut chunk = vec![0u8; 4096];

    loop {
        let n = match chan_stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                log::debug!("[terminal:ssh_agent] Agent forwarding channel read closed/error | error: {:#}", e);
                break;
            }
        };

        buf.extend_from_slice(&chunk[..n]);

        // Process all complete framed messages in the buffer
        while buf.len() >= 4 {
            let msg_len = u32::from_be_bytes(buf[0..4].try_into().unwrap()) as usize;
            let total_len = 4 + msg_len;
            if buf.len() < total_len {
                break;
            }

            let full_frame = &buf[..total_len];
            match local_client.exchange_raw(full_frame).await {
                Ok(resp_payload) => {
                    // Send back framed response [4-byte len][payload]
                    let mut framed_resp = Vec::with_capacity(4 + resp_payload.len());
                    framed_resp.extend_from_slice(&(resp_payload.len() as u32).to_be_bytes());
                    framed_resp.extend_from_slice(&resp_payload);

                    if let Err(e) = chan_stream.write_all(&framed_resp).await {
                        log::warn!("[terminal:ssh_agent] Failed to write agent response to channel | error: {:#}", e);
                        return Ok(());
                    }
                    let _ = chan_stream.flush().await;
                }
                Err(err) => {
                    log::warn!("[terminal:ssh_agent] SSH agent forwarding exchange error | error: {:#}", err);
                    // Send SSH_AGENT_FAILURE (5) back to remote
                    let failure_frame = vec![0, 0, 0, 1, super::protocol::SSH_AGENT_FAILURE];
                    let _ = chan_stream.write_all(&failure_frame).await;
                    let _ = chan_stream.flush().await;
                }
            }

            buf.drain(..total_len);
        }
    }

    Ok(())
}
