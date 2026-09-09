use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;
use velowork_state::{SshSession, TunnelKind, TunnelProfile, TunnelRuntimeInfo, TunnelStatus};
use crate::transport_pool::{ChannelType, ConnectionManager, ConnectionPolicy};

pub struct TunnelHandle {
    pub profile: TunnelProfile,
    stop_tx: watch::Sender<bool>,
    pub rx_bytes: Arc<AtomicU64>,
    pub tx_bytes: Arc<AtomicU64>,
    pub active_conns: Arc<AtomicUsize>,
    pub bound_port: Arc<AtomicU32>,
    pub status_rx: watch::Receiver<TunnelStatus>,
}

impl TunnelHandle {
    pub fn stop(&self) {
        let _ = self.stop_tx.send(true);
    }

    pub fn runtime_info(&self) -> TunnelRuntimeInfo {
        let port_val = self.bound_port.load(Ordering::Relaxed);
        let bound_port = if port_val > 0 { Some(port_val as u16) } else { None };
        TunnelRuntimeInfo {
            status: self.status_rx.borrow().clone(),
            rx_bytes: self.rx_bytes.load(Ordering::Relaxed),
            tx_bytes: self.tx_bytes.load(Ordering::Relaxed),
            active_connections: self.active_conns.load(Ordering::Relaxed),
            bound_port,
        }
    }
}

pub struct TunnelEngine {
    conn_mgr: Arc<ConnectionManager>,
    handles: std::sync::Mutex<std::collections::HashMap<String, TunnelHandle>>,
}

impl TunnelEngine {
    pub fn new(conn_mgr: Arc<ConnectionManager>) -> Self {
        Self {
            conn_mgr,
            handles: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub async fn start_tunnel(
        &self,
        profile: TunnelProfile,
        session: SshSession,
        jump_session: Option<SshSession>,
    ) -> anyhow::Result<()> {
        let tunnel_id = profile.id.clone();
        let (stop_tx, stop_rx) = watch::channel(false);
        let (status_tx, status_rx) = watch::channel(TunnelStatus::Running);

        let rx_bytes = Arc::new(AtomicU64::new(0));
        let tx_bytes = Arc::new(AtomicU64::new(0));
        let active_conns = Arc::new(AtomicUsize::new(0));
        let bound_port = Arc::new(AtomicU32::new(0));

        let handle = TunnelHandle {
            profile: profile.clone(),
            stop_tx,
            rx_bytes: rx_bytes.clone(),
            tx_bytes: tx_bytes.clone(),
            active_conns: active_conns.clone(),
            bound_port: bound_port.clone(),
            status_rx,
        };

        {
            let mut handles = self.handles.lock().unwrap();
            if let Some(old) = handles.remove(&tunnel_id) {
                old.stop();
            }
            handles.insert(tunnel_id.clone(), handle);
        }

        let conn_mgr = self.conn_mgr.clone();

        crate::pty_manager::get_tokio_runtime().spawn(async move {
            run_tunnel_loop(
                profile,
                session,
                jump_session,
                conn_mgr,
                stop_rx,
                status_tx,
                rx_bytes,
                tx_bytes,
                active_conns,
                bound_port,
            )
            .await;
        });

        Ok(())
    }

    pub fn stop_tunnel(&self, id: &str) {
        let mut handles = self.handles.lock().unwrap();
        if let Some(handle) = handles.remove(id) {
            handle.stop();
        }
        velowork_core::memory::trim_process_memory();
    }

    pub fn stop_all(&self) {
        let mut handles = self.handles.lock().unwrap();
        for (_, handle) in handles.drain() {
            handle.stop();
        }
        velowork_core::memory::trim_process_memory();
    }

    pub fn get_runtime_info(&self, id: &str) -> Option<TunnelRuntimeInfo> {
        let handles = self.handles.lock().unwrap();
        handles.get(id).map(|h| h.runtime_info())
    }
}

async fn run_tunnel_loop(
    profile: TunnelProfile,
    session: SshSession,
    jump_session: Option<SshSession>,
    conn_mgr: Arc<ConnectionManager>,
    mut stop_rx: watch::Receiver<bool>,
    status_tx: watch::Sender<TunnelStatus>,
    rx_bytes: Arc<AtomicU64>,
    tx_bytes: Arc<AtomicU64>,
    active_conns: Arc<AtomicUsize>,
    bound_port: Arc<AtomicU32>,
) {
    match &profile.kind {
        TunnelKind::Local { local_bind, remote_target } => {
            let listener = match TcpListener::bind(local_bind).await {
                Ok(l) => {
                    if let Ok(addr) = l.local_addr() {
                        bound_port.store(addr.port() as u32, Ordering::Relaxed);
                    }
                    l
                }
                Err(e) => {
                    let _ = status_tx.send(TunnelStatus::Error(format!("Failed to bind {}: {}", local_bind, e)));
                    return;
                }
            };
            let _ = status_tx.send(TunnelStatus::Running);

            let (host, port) = match parse_host_port(remote_target) {
                Some(hp) => hp,
                None => {
                    let _ = status_tx.send(TunnelStatus::Error(format!("Invalid remote target: {}", remote_target)));
                    return;
                }
            };

            loop {
                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            break;
                        }
                    }
                    accept_res = listener.accept() => {
                        match accept_res {
                            Ok((mut local_stream, _)) => {
                                let profile = profile.clone();
                                let session = session.clone();
                                let jump_session = jump_session.clone();
                                let conn_mgr = conn_mgr.clone();
                                let rx_bytes = rx_bytes.clone();
                                let tx_bytes = tx_bytes.clone();
                                let active_conns = active_conns.clone();
                                let host = host.clone();

                                crate::pty_manager::get_tokio_runtime().spawn(async move {
                                    active_conns.fetch_add(1, Ordering::Relaxed);
                                    let transport_res = get_transport_with_retry(&session, &conn_mgr, jump_session.as_ref()).await;

                                    if let Ok(transport) = transport_res {
                                        if let Ok(channel) = transport.handle.channel_open_direct_tcpip(&host, port as u32, "127.0.0.1", 0).await {
                                            let channel_type = ChannelType::Tunnel(profile.id.clone());
                                            transport.register_channel(channel_type.clone()).await;

                                            let mut channel_stream = channel.into_stream();
                                            let _ = copy_bidirectional_counted(&mut local_stream, &mut channel_stream, &rx_bytes, &tx_bytes).await;

                                            transport.unregister_channel(&channel_type).await;
                                        }
                                    }
                                    active_conns.fetch_sub(1, Ordering::Relaxed);
                                });
                            }
                            Err(_) => {
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                        }
                    }
                }
            }
        }
        TunnelKind::Dynamic { local_bind } => {
            let listener = match TcpListener::bind(local_bind).await {
                Ok(l) => {
                    if let Ok(addr) = l.local_addr() {
                        bound_port.store(addr.port() as u32, Ordering::Relaxed);
                    }
                    l
                }
                Err(e) => {
                    let _ = status_tx.send(TunnelStatus::Error(format!("Failed to bind SOCKS5 {}: {}", local_bind, e)));
                    return;
                }
            };
            let _ = status_tx.send(TunnelStatus::Running);

            loop {
                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            break;
                        }
                    }
                    accept_res = listener.accept() => {
                        match accept_res {
                            Ok((mut local_stream, _)) => {
                                let profile = profile.clone();
                                let session = session.clone();
                                let jump_session = jump_session.clone();
                                let conn_mgr = conn_mgr.clone();
                                let rx_bytes = rx_bytes.clone();
                                let tx_bytes = tx_bytes.clone();
                                let active_conns = active_conns.clone();

                                crate::pty_manager::get_tokio_runtime().spawn(async move {
                                    active_conns.fetch_add(1, Ordering::Relaxed);
                                    if let Ok((target_host, target_port)) = socks5_handshake(&mut local_stream).await {
                                        if let Ok(transport) = get_transport_with_retry(&session, &conn_mgr, jump_session.as_ref()).await {
                                            if let Ok(channel) = transport.handle.channel_open_direct_tcpip(&target_host, target_port as u32, "127.0.0.1", 0).await {
                                                // Send SOCKS5 reply success
                                                let reply = [0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
                                                if local_stream.write_all(&reply).await.is_ok() {
                                                    let channel_type = ChannelType::Tunnel(profile.id.clone());
                                                    transport.register_channel(channel_type.clone()).await;

                                                    let mut channel_stream = channel.into_stream();
                                                    let _ = copy_bidirectional_counted(&mut local_stream, &mut channel_stream, &rx_bytes, &tx_bytes).await;

                                                    transport.unregister_channel(&channel_type).await;
                                                }
                                            }
                                        }
                                    }
                                    active_conns.fetch_sub(1, Ordering::Relaxed);
                                });
                            }
                            Err(_) => {
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                        }
                    }
                }
            }
        }
        TunnelKind::Remote { remote_bind, local_target: _ } => {
            let (r_host, r_port) = match parse_host_port(remote_bind) {
                Some(hp) => hp,
                None => ("0.0.0.0".to_string(), 8080),
            };

            let transport = match get_transport_with_retry(&session, &conn_mgr, jump_session.as_ref()).await {
                Ok(t) => t,
                Err(e) => {
                    let _ = status_tx.send(TunnelStatus::Error(format!("Failed to connect SSH: {}", e)));
                    return;
                }
            };

            // Global request tcpip forward
            let req_res = transport.handle.tcpip_forward(&r_host, r_port as u32).await;
            if req_res.is_err() {
                let _ = status_tx.send(TunnelStatus::Error("Remote port forwarding request rejected by server".to_string()));
                return;
            }

            let _ = status_tx.send(TunnelStatus::Running);
            let channel_type = ChannelType::Tunnel(profile.id.clone());
            transport.register_channel(channel_type.clone()).await;

            loop {
                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            break;
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {
                        if !transport.is_alive().await {
                            let _ = status_tx.send(TunnelStatus::Error("SSH connection dropped".to_string()));
                            break;
                        }
                    }
                }
            }

            transport.unregister_channel(&channel_type).await;
        }
    }
    let _ = status_tx.send(TunnelStatus::Stopped);
}

async fn get_transport_with_retry(
    session: &SshSession,
    conn_mgr: &ConnectionManager,
    jump_session: Option<&SshSession>,
) -> anyhow::Result<Arc<crate::transport_pool::SharedTransport>> {
    let mut attempts = 0;
    loop {
        match conn_mgr.get_or_connect(session, ConnectionPolicy::Reuse, jump_session).await {
            Ok(t) => return Ok(t),
            Err(e) => {
                attempts += 1;
                if attempts >= 3 {
                    return Err(e);
                }
                tokio::time::sleep(Duration::from_millis(1000 * attempts)).await;
            }
        }
    }
}

fn parse_host_port(target: &str) -> Option<(String, u16)> {
    if let Ok(addr) = target.parse::<SocketAddr>() {
        return Some((addr.ip().to_string(), addr.port()));
    }
    let parts: Vec<&str> = target.split(':').collect();
    if parts.len() == 2 {
        let host = parts[0].to_string();
        if let Ok(port) = parts[1].parse::<u16>() {
            return Some((host, port));
        }
    }
    None
}

async fn socks5_handshake<S: AsyncReadExt + AsyncWriteExt + Unpin>(
    stream: &mut S,
) -> anyhow::Result<(String, u16)> {
    let mut ver_nmethods = [0u8; 2];
    stream.read_exact(&mut ver_nmethods).await?;
    if ver_nmethods[0] != 0x05 {
        return Err(anyhow::anyhow!("Invalid SOCKS version"));
    }
    let nmethods = ver_nmethods[1] as usize;
    let mut methods = vec![0u8; nmethods];
    stream.read_exact(&mut methods).await?;

    // Response no auth
    stream.write_all(&[0x05, 0x00]).await?;

    let mut req_header = [0u8; 4];
    stream.read_exact(&mut req_header).await?;
    if req_header[0] != 0x05 || req_header[1] != 0x01 {
        return Err(anyhow::anyhow!("Unsupported SOCKS5 command"));
    }

    let target_host = match req_header[3] {
        1 => {
            // IPv4
            let mut ip = [0u8; 4];
            stream.read_exact(&mut ip).await?;
            format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
        }
        3 => {
            // Domain name
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await?;
            let mut domain = vec![0u8; len[0] as usize];
            stream.read_exact(&mut domain).await?;
            String::from_utf8(domain)?
        }
        4 => {
            // IPv6
            let mut ip = [0u8; 16];
            stream.read_exact(&mut ip).await?;
            "::1".to_string()
        }
        _ => return Err(anyhow::anyhow!("Invalid SOCKS5 address type")),
    };

    let mut port_bytes = [0u8; 2];
    stream.read_exact(&mut port_bytes).await?;
    let target_port = u16::from_be_bytes(port_bytes);

    Ok((target_host, target_port))
}

async fn copy_bidirectional_counted<S1, S2>(
    s1: &mut S1,
    s2: &mut S2,
    rx_bytes: &Arc<AtomicU64>,
    tx_bytes: &Arc<AtomicU64>,
) -> anyhow::Result<()>
where
    S1: AsyncReadExt + AsyncWriteExt + Unpin,
    S2: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let (mut r1, mut w1) = tokio::io::split(s1);
    let (mut r2, mut w2) = tokio::io::split(s2);

    let rx = rx_bytes.clone();
    let tx = tx_bytes.clone();

    let c1 = async move {
        let mut buf = [0u8; 8192];
        loop {
            let n = r1.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            w2.write_all(&buf[..n]).await?;
            tx.fetch_add(n as u64, Ordering::Relaxed);
        }
        w2.shutdown().await?;
        Ok::<(), anyhow::Error>(())
    };

    let c2 = async move {
        let mut buf = [0u8; 8192];
        loop {
            let n = r2.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            w1.write_all(&buf[..n]).await?;
            rx.fetch_add(n as u64, Ordering::Relaxed);
        }
        w1.shutdown().await?;
        Ok::<(), anyhow::Error>(())
    };

    tokio::try_join!(c1, c2)?;
    Ok(())
}
