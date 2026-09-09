use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use velowork_state::{SessionId, SshSession, TunnelId};
use crate::pty_manager::{connect_ssh_handle, SshClient};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelType {
    Terminal(String),
    Sftp,
    Tunnel(TunnelId),
    Agent(String),
    Monitor,
}

pub struct SharedTransport {
    pub session_id: SessionId,
    pub handle: russh::client::Handle<SshClient>,
    pub active_channels: Arc<Mutex<Vec<ChannelType>>>,
    pub last_used: Arc<Mutex<Instant>>,
}

impl SharedTransport {
    pub fn new(session_id: SessionId, handle: russh::client::Handle<SshClient>) -> Self {
        Self {
            session_id,
            handle,
            active_channels: Arc::new(Mutex::new(Vec::new())),
            last_used: Arc::new(Mutex::new(Instant::now())),
        }
    }

    pub async fn channel_count(&self) -> usize {
        self.active_channels.lock().await.len()
    }

    pub async fn register_channel(&self, channel: ChannelType) {
        let mut channels = self.active_channels.lock().await;
        channels.push(channel);
        *self.last_used.lock().await = Instant::now();
    }

    pub async fn unregister_channel(&self, channel_matcher: &ChannelType) {
        let mut channels = self.active_channels.lock().await;
        channels.retain(|c| c != channel_matcher);
        *self.last_used.lock().await = Instant::now();
    }

    pub async fn is_alive(&self) -> bool {
        !self.handle.is_closed()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionPolicy {
    Reuse,
    New,
}

pub struct ConnectionManager {
    transports: Arc<Mutex<HashMap<SessionId, Arc<SharedTransport>>>>,
    idle_timeout: Duration,
}

impl ConnectionManager {
    pub fn new(idle_timeout: Duration) -> Self {
        let manager = Self {
            transports: Arc::new(Mutex::new(HashMap::new())),
            idle_timeout,
        };
        manager.spawn_idle_cleaner();
        manager
    }

    pub async fn get_or_connect(
        &self,
        session: &SshSession,
        policy: ConnectionPolicy,
        jump_session: Option<&SshSession>,
    ) -> anyhow::Result<Arc<SharedTransport>> {
        let mut pool = self.transports.lock().await;

        if policy == ConnectionPolicy::Reuse {
            if let Some(transport) = pool.get(&session.id) {
                if transport.is_alive().await {
                    return Ok(transport.clone());
                }
            }
        }

        let handle = connect_ssh_handle(session, jump_session).await?;
        let shared = Arc::new(SharedTransport::new(session.id.clone(), handle));

        if policy == ConnectionPolicy::Reuse {
            pool.insert(session.id.clone(), shared.clone());
        }

        Ok(shared)
    }

    pub async fn remove_transport(&self, session_id: &str) {
        let mut pool = self.transports.lock().await;
        if let Some(transport) = pool.remove(session_id) {
            let _ = transport
                .handle
                .disconnect(russh::Disconnect::ByApplication, "Explicit removal", "en")
                .await;
        }
    }

    fn spawn_idle_cleaner(&self) {
        let transports = self.transports.clone();
        let timeout = self.idle_timeout;

        crate::pty_manager::get_tokio_runtime().spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let mut pool = transports.lock().await;
                let mut to_remove = Vec::new();

                for (id, transport) in pool.iter() {
                    if transport.channel_count().await == 0 {
                        let elapsed = transport.last_used.lock().await.elapsed();
                        if elapsed >= timeout {
                            to_remove.push(id.clone());
                        }
                    }
                }

                for id in to_remove {
                    if let Some(transport) = pool.remove(&id) {
                        let _ = transport
                            .handle
                            .disconnect(russh::Disconnect::ByApplication, "Idle timeout", "en")
                            .await;
                    }
                }
            }
        });
    }
}
