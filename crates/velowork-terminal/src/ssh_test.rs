//! SSH connection testing functionality.

use anyhow::Result;
use crate::pty_manager::authenticate_with_private_key;
use russh::client;
use std::sync::Arc;
use std::time::{Duration, Instant};
use velowork_state::{SshAuthType, SshTestResult};

/// Test SSH connection (blocking, runs in a separate thread with tokio runtime)
pub fn test_ssh_connection_blocking(
    host: &str,
    port: u16,
    username: &str,
    auth: &SshAuthType,
    timeout_secs: u32,
    session_config: Option<&velowork_state::SshSession>,
) -> SshTestResult {
    let start = Instant::now();

    // 创建 tokio 运行时
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            return SshTestResult {
                success: false,
                latency_ms: start.elapsed().as_millis() as u64,
                server_version: None,
                error: Some(format!("Failed to create runtime: {}", e)),
            };
        }
    };

    // 在 tokio 运行时中执行测试
    rt.block_on(async {
        // 创建 SSH 配置
        let config = Arc::new(russh::client::Config {
            inactivity_timeout: Some(Duration::from_secs(timeout_secs as u64)),
            ..Default::default()
        });

        // 尝试连接 Socket (支持 Direct / SOCKS5 / HTTP Proxy / Jump)
        let jump_session = session_config
            .and_then(|sc| {
                if sc.proxy_type == velowork_state::ProxyType::Jump {
                    sc.jump_session_id.as_ref().and_then(|jid| {
                        velowork_core::storage::database().and_then(|db| {
                            let conn = db.conn();
                            conn.query_row(
                                "SELECT payload FROM session_tree_node WHERE id = ?1 AND kind = 'session'",
                                &[jid.as_str()],
                                |row| row.get::<_, String>(0),
                            ).ok().and_then(|payload| serde_json::from_str::<velowork_state::SshSession>(&payload).ok())
                        })
                    })
                } else {
                    None
                }
            });
        let is_jump = session_config.map(|s| s.proxy_type) == Some(velowork_state::ProxyType::Jump);
        let stream_result = if is_jump {
            let jump = jump_session.as_ref()
                .ok_or_else(|| anyhow::anyhow!("Jump host session not configured"));
            match jump {
                Ok(j) => crate::pty_manager::connect_jump_stream(host, port, j).await,
                Err(e) => Err(e),
            }
        } else {
            crate::pty_manager::connect_tcp_or_proxy_stream(host, port, session_config).await
        };
        let stream = match stream_result {
            Ok(s) => s,
            Err(e) => {
                return SshTestResult {
                    success: false,
                    latency_ms: start.elapsed().as_millis() as u64,
                    server_version: None,
                    error: Some(format!("Network / Proxy connection failed: {}", e)),
                };
            }
        };

        let handler = TestSshHandler {
            server_version: None,
        };

        match tokio::time::timeout(
            Duration::from_secs(timeout_secs as u64),
            client::connect_stream(config, stream, handler),
        )
        .await
        {
            Ok(Ok(mut session)) => {
                let latency = start.elapsed().as_millis() as u64;

                // 尝试认证（仅验证凭证格式，不实际完成认证）
                let auth_result = match auth {
                    SshAuthType::Password { password } => {
                        if let Some(pwd) = password {
                            match session.authenticate_password(username, pwd).await {
                                Ok(auth) if auth.success() => Ok(()),
                                Ok(_) => Err("Authentication failed: invalid password".to_string()),
                                Err(e) => Err(format!("Password auth error: {}", e)),
                            }
                        } else {
                            // No password provided, just verify connection path
                            Ok(())
                        }
                    }
                    SshAuthType::PrivateKey { key_path, passphrase } => {
                        // 复用实际连接同一套私钥认证逻辑：加载私钥并协商 RSA
                        // 签名 hash 算法后真正执行公钥认证，而非只校验文件格式，
                        // 确保测试结果能如实反映能否登录。
                        authenticate_with_private_key(
                            &mut session,
                            username,
                            key_path,
                            passphrase.as_deref(),
                        )
                        .await
                    }
                    SshAuthType::KeyboardInteractive => {
                        // 键盘交互认证需要用户输入，测试连接只验证连通性
                        Ok(())
                    }
                    SshAuthType::SshAgent { socket_path } => {
                        match crate::ssh_agent::authenticate_with_agent(
                            &mut session,
                            username,
                            socket_path.as_deref(),
                        )
                        .await
                        {
                            Ok(true) => Ok(()),
                            Ok(false) => Err(
                                "SSH Agent authentication failed (no keys accepted by server or agent is empty)"
                                    .to_string(),
                            ),
                            Err(e) => Err(format!("SSH Agent error: {}", e)),
                        }
                    }
                };

                // 关闭连接
                let _ = session.disconnect(russh::Disconnect::ByApplication, "", "").await;

                match auth_result {
                    Ok(_) => SshTestResult {
                        success: true,
                        latency_ms: latency,
                        server_version: None,
                        error: None,
                    },
                    Err(e) => SshTestResult {
                        success: false,
                        latency_ms: latency,
                        server_version: None,
                        error: Some(e),
                    },
                }
            }
            Ok(Err(e)) => SshTestResult {
                success: false,
                latency_ms: start.elapsed().as_millis() as u64,
                server_version: None,
                error: Some(format!("Connection failed: {}", e)),
            },
            Err(_) => SshTestResult {
                success: false,
                latency_ms: start.elapsed().as_millis() as u64,
                server_version: None,
                error: Some("Connection timed out".to_string()),
            },
        }
    })
}

/// SSH handler for testing connections
struct TestSshHandler {
    #[allow(dead_code)]
    server_version: Option<String>,
}

impl client::Handler for TestSshHandler {
    type Error = russh::Error;

    fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        // 测试连接时接受所有服务器密钥
        async { Ok(true) }
    }
}
