//! SSH-backed [`MonitorSource`] for remote host metrics.
//!
//! Reuses the *already-established* russh session of a focused terminal pane
//! (no second dial), opening a throwaway exec channel per probe and draining
//! its output. The exact same probe script the local source runs works here
//! unchanged — only the transport differs.

use crate::pty_manager::get_tokio_runtime;
use crate::pty_manager::SshClient;
use std::sync::Arc;
use velowork_monitor::{MonitorError, MonitorSource, Result};

/// Shared handle to a live russh session, keyed by terminal.
pub type SshSessionHandle = Arc<russh::client::Handle<SshClient>>;

/// A [`MonitorSource`] that runs the probe over an existing SSH session.
pub struct SshMonitorSource {
    session: SshSessionHandle,
}

impl SshMonitorSource {
    pub fn new(session: SshSessionHandle) -> Self {
        Self { session }
    }
}

impl MonitorSource for SshMonitorSource {
    fn exec(&self, script: &str) -> Result<String> {
        let session = self.session.clone();
        let script = script.to_string();

        // The session's reader loop runs on the shared tokio runtime; we spawn the
        // exec future there and block this (collector) thread until it resolves.
        let rt = get_tokio_runtime();
        let handle = rt.spawn(async move {
            let mut channel = session
                .channel_open_session()
                .await
                .map_err(|e| MonitorError::Exec(format!("ssh channel open failed: {}", e)))?;

            // `req = false`: no PTY needed for scripted probes.
            channel
                .exec(false, script.as_str())
                .await
                .map_err(|e| MonitorError::Exec(format!("ssh exec failed: {}", e)))?;

            let mut out = String::new();
            loop {
                match channel.wait().await {
                    Some(russh::ChannelMsg::Data { data }) => {
                        out.push_str(&String::from_utf8_lossy(&data.to_vec()));
                    }
                    Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                        out.push_str(&String::from_utf8_lossy(&data.to_vec()));
                    }
                    Some(russh::ChannelMsg::Eof)
                    | Some(russh::ChannelMsg::Close)
                    | None => break,
                    Some(_) => {}
                }
            }
            Ok::<_, MonitorError>(out)
        });

        match rt.block_on(handle) {
            Ok(res) => res,
            Err(e) => Err(MonitorError::Exec(format!(
                "ssh monitor task panicked: {}",
                e
            ))),
        }
    }
}
