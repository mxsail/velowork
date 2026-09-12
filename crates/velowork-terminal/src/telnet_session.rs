//! 跨平台 Telnet (RFC 854) 异步会话实现。
//!
//! 支持标准 IAC 命令解析、NAWS（窗口大小自动协商）、Echo/Suppress Go-Ahead
//! 选项协商与双向字符流转发。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use async_channel::Sender;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::pty_manager::PtyEvent;

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;

const OPT_ECHO: u8 = 1;
const OPT_SUPPRESS_GO_AHEAD: u8 = 3;
const OPT_TERMINAL_TYPE: u8 = 24;
const OPT_NAWS: u8 = 31;

const TELNET_IS: u8 = 0;
const TELNET_SEND: u8 = 1;

/// Telnet 会话配置。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TelnetConfig {
    /// 目标主机名或 IP 地址。
    pub host: String,
    /// 目标端口（默认 23）。
    #[serde(default = "default_telnet_port")]
    pub port: u16,
    /// 字符编码（"utf-8"、"gbk"、"latin1"）。
    #[serde(default = "default_encoding")]
    pub encoding: String,
    /// 终端类型（如 "xterm-256color"、"vt100"）。
    #[serde(default = "default_term_type")]
    pub term_type: String,
    /// 连接建立后自动执行的启动命令
    #[serde(default)]
    pub startup_command: Option<String>,
}

fn default_telnet_port() -> u16 {
    23
}
fn default_encoding() -> String {
    "utf-8".to_string()
}
fn default_term_type() -> String {
    velowork_core::DEFAULT_TERM_TYPE.to_string()
}

impl Default for TelnetConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: default_telnet_port(),
            encoding: default_encoding(),
            term_type: default_term_type(),
            startup_command: None,
        }
    }
}

/// Telnet 协议解析状态机。
#[derive(Debug, PartialEq, Eq)]
enum TelnetState {
    Normal,
    Iac,
    Will,
    Wont,
    Do,
    Dont,
    Subneg,
    SubnegIac,
}

/// 运行异步 Telnet 会话（直连）。
pub async fn run_telnet_session(
    terminal_id: String,
    config: TelnetConfig,
    event_tx: Sender<PtyEvent>,
    input_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    resize_rx: tokio::sync::mpsc::Receiver<(u16, u16)>,
    exit_signal: Arc<AtomicBool>,
) -> Result<()> {
    let addr = format!("{}:{}", config.host, config.port);
    let stream = match TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("\r\n\x1b[31m[Failed to connect to Telnet host '{addr}': {e}]\x1b[0m\r\n");
            let _ = event_tx.send(PtyEvent::Data {
                terminal_id: terminal_id.clone(),
                data: msg.into_bytes(),
            }).await;
            let _ = event_tx.send(PtyEvent::Exit {
                terminal_id,
                exit_code: Some(1),
            }).await;
            return Err(anyhow::anyhow!("Failed to connect to Telnet host '{addr}': {e}"));
        }
    };

    run_telnet_session_with_stream(
        terminal_id,
        config,
        Box::new(stream),
        event_tx,
        input_rx,
        resize_rx,
        exit_signal,
    )
    .await
}

/// 基于已建立的异步双向流（支持代理或直连）运行 Telnet 会话。
pub async fn run_telnet_session_with_stream(
    terminal_id: String,
    config: TelnetConfig,
    stream: Box<dyn crate::pty_manager::AsyncReadWrite>,
    event_tx: Sender<PtyEvent>,
    mut input_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    mut resize_rx: tokio::sync::mpsc::Receiver<(u16, u16)>,
    exit_signal: Arc<AtomicBool>,
) -> Result<()> {
    let addr = format!("{}:{}", config.host, config.port);
    let (mut reader, mut writer) = tokio::io::split(stream);

    let mut state = TelnetState::Normal;
    let mut read_buf = [0u8; 4096];
    let mut subneg_buf = Vec::new();
    let mut naws_supported = false;
    let mut current_cols: u16 = 80;
    let mut current_rows: u16 = 24;

    loop {
        if exit_signal.load(Ordering::Relaxed) {
            break;
        }

        tokio::select! {
            // 从网络读取 Telnet 数据并解析 IAC
            read_res = reader.read(&mut read_buf) => {
                match read_res {
                    Ok(0) => {
                        log::info!("[terminal:telnet] Telnet connection closed by remote | addr={}", addr);
                        break;
                    }
                    Ok(n) => {
                        let mut clean_data = Vec::with_capacity(n);
                        let mut response_buf = Vec::new();

                        for &b in &read_buf[..n] {
                            match state {
                                TelnetState::Normal => {
                                    if b == IAC {
                                        state = TelnetState::Iac;
                                    } else {
                                        clean_data.push(b);
                                    }
                                }
                                TelnetState::Iac => {
                                    match b {
                                        IAC => {
                                            clean_data.push(IAC);
                                            state = TelnetState::Normal;
                                        }
                                        WILL => state = TelnetState::Will,
                                        WONT => state = TelnetState::Wont,
                                        DO => state = TelnetState::Do,
                                        DONT => state = TelnetState::Dont,
                                        SB => {
                                            subneg_buf.clear();
                                            state = TelnetState::Subneg;
                                        }
                                        _ => state = TelnetState::Normal,
                                    }
                                }
                                TelnetState::Will => {
                                    match b {
                                        OPT_ECHO | OPT_SUPPRESS_GO_AHEAD => {
                                            response_buf.extend_from_slice(&[IAC, DO, b]);
                                        }
                                        _ => {
                                            response_buf.extend_from_slice(&[IAC, DONT, b]);
                                        }
                                    }
                                    state = TelnetState::Normal;
                                }
                                TelnetState::Wont => {
                                    response_buf.extend_from_slice(&[IAC, DONT, b]);
                                    state = TelnetState::Normal;
                                }
                                TelnetState::Do => {
                                    match b {
                                        OPT_SUPPRESS_GO_AHEAD => {
                                            response_buf.extend_from_slice(&[IAC, WILL, b]);
                                        }
                                        OPT_NAWS => {
                                            naws_supported = true;
                                            response_buf.extend_from_slice(&[IAC, WILL, OPT_NAWS]);
                                            // 发送初始窗口大小
                                            let (c_hi, c_lo) = ((current_cols >> 8) as u8, (current_cols & 0xff) as u8);
                                            let (r_hi, r_lo) = ((current_rows >> 8) as u8, (current_rows & 0xff) as u8);
                                            response_buf.extend_from_slice(&[IAC, SB, OPT_NAWS, c_hi, c_lo, r_hi, r_lo, IAC, SE]);
                                        }
                                        OPT_TERMINAL_TYPE => {
                                            response_buf.extend_from_slice(&[IAC, WILL, OPT_TERMINAL_TYPE]);
                                        }
                                        _ => {
                                            response_buf.extend_from_slice(&[IAC, WONT, b]);
                                        }
                                    }
                                    state = TelnetState::Normal;
                                }
                                TelnetState::Dont => {
                                    response_buf.extend_from_slice(&[IAC, WONT, b]);
                                    state = TelnetState::Normal;
                                }
                                TelnetState::Subneg => {
                                    if b == IAC {
                                        state = TelnetState::SubnegIac;
                                    } else {
                                        subneg_buf.push(b);
                                    }
                                }
                                TelnetState::SubnegIac => {
                                    if b == SE {
                                        // RFC 1091: IAC SB TERMINAL-TYPE SEND IAC SE -> IAC SB TERMINAL-TYPE IS <type> IAC SE
                                        if subneg_buf.first() == Some(&OPT_TERMINAL_TYPE)
                                            && subneg_buf.get(1) == Some(&TELNET_SEND)
                                        {
                                            response_buf.extend_from_slice(&[IAC, SB, OPT_TERMINAL_TYPE, TELNET_IS]);
                                            response_buf.extend_from_slice(config.term_type.as_bytes());
                                            response_buf.extend_from_slice(&[IAC, SE]);
                                        }
                                        subneg_buf.clear();
                                        state = TelnetState::Normal;
                                    } else {
                                        subneg_buf.push(IAC);
                                        subneg_buf.push(b);
                                        state = TelnetState::Subneg;
                                    }
                                }
                            }
                        }

                        if !response_buf.is_empty() {
                            let _ = writer.write_all(&response_buf).await;
                        }

                        if !clean_data.is_empty() {
                            let processed_data = crate::pty_manager::transcode_to_utf8(&clean_data, &config.encoding);
                            if event_tx.send(PtyEvent::Data { terminal_id: terminal_id.clone(), data: processed_data }).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("[terminal:telnet] Telnet read error | addr={} | error: {:#}", addr, e);
                        break;
                    }
                }
            }

            // 从终端接收键盘输入并写入网络
            input_opt = input_rx.recv() => {
                match input_opt {
                    Some(data) => {
                        let target_bytes = crate::pty_manager::transcode_from_utf8(&data, &config.encoding);
                        // 如果包含 0xFF，进行转义 (0xFF -> 0xFF 0xFF)
                        let mut encoded = Vec::with_capacity(target_bytes.len());
                        for &b in &target_bytes {
                            if b == IAC {
                                encoded.push(IAC);
                                encoded.push(IAC);
                            } else {
                                encoded.push(b);
                            }
                        }
                        if let Err(e) = writer.write_all(&encoded).await {
                            log::warn!("[terminal:telnet] Telnet write error | addr={} | error: {:#}", addr, e);
                            break;
                        }
                        let _ = writer.flush().await;
                    }
                    None => break,
                }
            }

            // 窗口大小变更通知 (NAWS)
            resize_opt = resize_rx.recv() => {
                if let Some((cols, rows)) = resize_opt {
                    current_cols = cols;
                    current_rows = rows;
                    if naws_supported {
                        let (c_hi, c_lo) = ((cols >> 8) as u8, (cols & 0xff) as u8);
                        let (r_hi, r_lo) = ((rows >> 8) as u8, (rows & 0xff) as u8);
                        let naws_pkt = [IAC, SB, OPT_NAWS, c_hi, c_lo, r_hi, r_lo, IAC, SE];
                        let _ = writer.write_all(&naws_pkt).await;
                    }
                }
            }
        }
    }

    let _ = event_tx.send(PtyEvent::Exit { terminal_id, exit_code: None }).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telnet_config_defaults() {
        let cfg = TelnetConfig::default();
        assert_eq!(cfg.port, 23);
        assert_eq!(cfg.encoding, "utf-8");
        assert_eq!(cfg.term_type, velowork_core::DEFAULT_TERM_TYPE);
    }

    #[tokio::test]
    async fn test_telnet_session_connection_failure() {
        let (event_tx, event_rx) = async_channel::unbounded();
        let (_input_tx, input_rx) = tokio::sync::mpsc::channel(1);
        let (_resize_tx, resize_rx) = tokio::sync::mpsc::channel(1);
        let exit_signal = Arc::new(AtomicBool::new(false));

        let config = TelnetConfig {
            host: "127.0.0.1".to_string(),
            port: 59999,
            ..Default::default()
        };

        let res = run_telnet_session("telnet-term-1".to_string(), config, event_tx, input_rx, resize_rx, exit_signal).await;
        assert!(res.is_err());

        let ev1 = event_rx.recv().await.unwrap();
        match ev1 {
            PtyEvent::Data { terminal_id, data } => {
                assert_eq!(terminal_id, "telnet-term-1");
                let text = String::from_utf8_lossy(&data);
                assert!(text.contains("Failed to connect to Telnet host '127.0.0.1:59999'"));
            }
            _ => panic!("Expected PtyEvent::Data"),
        }

        let ev2 = event_rx.recv().await.unwrap();
        match ev2 {
            PtyEvent::Exit { terminal_id, exit_code } => {
                assert_eq!(terminal_id, "telnet-term-1");
                assert_eq!(exit_code, Some(1));
            }
            _ => panic!("Expected PtyEvent::Exit"),
        }
    }
}
