//! 跨平台串口 (Serial/UART) 异步会话实现。
//!
//! 支持自动端口枚举、常用波特率与数据位/停止位/校验位/流控配置，
//! 以及 DTR / RTS 硬件引脚控制与异步双向字符流转发。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_channel::Sender;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_serial::{DataBits, FlowControl, Parity, SerialPort, SerialPortBuilderExt, StopBits};

use crate::pty_manager::PtyEvent;

/// 串口设备信息描述（用于 UI 端口下拉列表展示）。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SerialPortDescription {
    /// 端口路径/名称（如 `COM3`、`/dev/ttyUSB0`、`/dev/cu.usbserial-10`）。
    pub port_name: String,
    /// 用户友好描述信息（如芯片型号、制造商）。
    pub description: Option<String>,
    /// USB 厂商 ID（十六进制格式，如 `10c4`）。
    pub vid: Option<String>,
    /// USB 产品 ID（十六进制格式，如 `ea60`）。
    pub pid: Option<String>,
    /// 设备序列号。
    pub serial_number: Option<String>,
}

impl SerialPortDescription {
    /// 获取人类可读的展示名称（包含设备型号信息）。
    pub fn display_label(&self) -> String {
        let mut details: Vec<String> = Vec::new();
        if let Some(ref desc) = self.description
            && !desc.is_empty()
        {
            details.push(desc.clone());
        }
        if let (Some(vid), Some(pid)) = (&self.vid, &self.pid)
            && details.is_empty()
        {
            details.push(format!("VID:{vid} PID:{pid}"));
        }
        if details.is_empty() {
            self.port_name.clone()
        } else {
            format!("{} ({})", self.port_name, details.join(" - "))
        }
    }
}

/// 枚举当前系统所有可用的串口设备。
pub fn list_available_serial_ports() -> Vec<SerialPortDescription> {
    let ports = match serialport::available_ports() {
        Ok(p) => p,
        Err(err) => {
            log::warn!("[terminal:serial] Failed to list available serial ports | error: {:#}", err);
            Vec::new()
        }
    };

    ports
        .into_iter()
        .map(|p| {
            let (description, vid, pid, serial_number) = match p.port_type {
                serialport::SerialPortType::UsbPort(usb) => (
                    usb.product,
                    Some(format!("{:04x}", usb.vid)),
                    Some(format!("{:04x}", usb.pid)),
                    usb.serial_number,
                ),
                serialport::SerialPortType::PciPort => (Some("PCI Port".to_string()), None, None, None),
                serialport::SerialPortType::BluetoothPort => (Some("Bluetooth Port".to_string()), None, None, None),
                serialport::SerialPortType::Unknown => (None, None, None, None),
            };

            SerialPortDescription {
                port_name: p.port_name,
                description,
                vid,
                pid,
                serial_number,
            }
        })
        .collect()
}

/// 串口会话配置。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SerialConfig {
    /// 端口名称（如 `COM3`、`/dev/ttyUSB0`）。
    pub port: String,
    /// 波特率（常见 115200、9600、57600 等）。
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    /// 数据位（5、6、7、8，默认 8）。
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    /// 停止位（1、2，默认 1）。
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    /// 校验位（"none"、"even"、"odd"、"mark"、"space"，默认 "none"）。
    #[serde(default = "default_parity")]
    pub parity: String,
    /// 流控制（"none"、"software"、"hardware"，默认 "none"）。
    #[serde(default = "default_flow_control")]
    pub flow_control: String,
    /// 数据终端就绪 DTR 状态。
    #[serde(default = "default_true")]
    pub dtr: bool,
    /// 请求发送 RTS 状态。
    #[serde(default = "default_true")]
    pub rts: bool,
    /// 字符集编码（默认 UTF-8）。
    #[serde(default)]
    pub charset: Option<String>,
    /// 显示模式："text" 或 "hex"
    #[serde(default = "default_serial_display_mode")]
    pub display_mode: String,
    /// 行尾换行符："crlf"、"lf" 或 "cr"
    #[serde(default = "default_serial_line_ending")]
    pub line_ending: String,
    /// 本地字符回显
    #[serde(default)]
    pub local_echo: bool,
    /// 时间戳显示
    #[serde(default)]
    pub timestamps: bool,
    /// 掉线自动重连
    #[serde(default)]
    pub auto_reconnect: bool,
}

fn default_baud_rate() -> u32 {
    115200
}
fn default_data_bits() -> u8 {
    8
}
fn default_stop_bits() -> u8 {
    1
}
fn default_parity() -> String {
    "none".to_string()
}
fn default_flow_control() -> String {
    "none".to_string()
}
fn default_true() -> bool {
    true
}
fn default_serial_display_mode() -> String {
    "text".to_string()
}
fn default_serial_line_ending() -> String {
    "crlf".to_string()
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port: String::new(),
            baud_rate: default_baud_rate(),
            data_bits: default_data_bits(),
            stop_bits: default_stop_bits(),
            parity: default_parity(),
            flow_control: default_flow_control(),
            dtr: true,
            rts: true,
            charset: None,
            display_mode: default_serial_display_mode(),
            line_ending: default_serial_line_ending(),
            local_echo: false,
            timestamps: false,
            auto_reconnect: false,
        }
    }
}

impl SerialConfig {
    /// 解析数据位为 tokio_serial 的 DataBits。
    pub fn resolved_data_bits(&self) -> DataBits {
        match self.data_bits {
            5 => DataBits::Five,
            6 => DataBits::Six,
            7 => DataBits::Seven,
            _ => DataBits::Eight,
        }
    }

    /// 解析停止位为 tokio_serial 的 StopBits。
    pub fn resolved_stop_bits(&self) -> StopBits {
        match self.stop_bits {
            2 => StopBits::Two,
            _ => StopBits::One,
        }
    }

    /// 解析校验位为 tokio_serial 的 Parity。
    pub fn resolved_parity(&self) -> Parity {
        match self.parity.to_lowercase().as_str() {
            "even" => Parity::Even,
            "odd" => Parity::Odd,
            _ => Parity::None,
        }
    }

    /// 解析流控为 tokio_serial 的 FlowControl。
    pub fn resolved_flow_control(&self) -> FlowControl {
        match self.flow_control.to_lowercase().as_str() {
            "software" => FlowControl::Software,
            "hardware" => FlowControl::Hardware,
            _ => FlowControl::None,
        }
    }

    /// 格式化为简短的调试摘要（如 `115200 8-N-1`）。
    pub fn summary_label(&self) -> String {
        let p_char = match self.parity.to_lowercase().as_str() {
            "even" => 'E',
            "odd" => 'O',
            _ => 'N',
        };
        format!("{} {}-{}-{}", self.baud_rate, self.data_bits, p_char, self.stop_bits)
    }
}

/// 将输入数据中的换行符规整替换为目标 line_ending 字节序列
pub fn format_serial_input_line_ending(input: &[u8], line_ending: &str) -> Vec<u8> {
    let target = match line_ending.to_lowercase().as_str() {
        "lf" => b"\n".as_slice(),
        "cr" => b"\r".as_slice(),
        _ => b"\r\n".as_slice(),
    };
    let mut out = Vec::with_capacity(input.len() + 8);
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'\r' {
            if i + 1 < input.len() && input[i + 1] == b'\n' {
                i += 2;
            } else {
                i += 1;
            }
            out.extend_from_slice(target);
        } else if input[i] == b'\n' {
            i += 1;
            out.extend_from_slice(target);
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
    out
}

fn current_time_hms() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (s % 86400) / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    format!("[{h:02}:{m:02}:{sec:02}] ")
}

/// 格式化接收到的串口原始数据（支持 Hex 十六进制展示及行首时间戳）
pub fn format_serial_output_data(
    raw: &[u8],
    charset: &str,
    display_mode: &str,
    timestamps: bool,
    at_line_start: &mut bool,
) -> Vec<u8> {
    if display_mode.eq_ignore_ascii_case("hex") {
        let mut s = String::new();
        for b in raw {
            s.push_str(&format!("{:02X} ", b));
        }
        return s.into_bytes();
    }

    let utf8_text = crate::pty_manager::transcode_to_utf8(raw, charset);
    if !timestamps {
        return utf8_text;
    }

    let text_str = String::from_utf8_lossy(&utf8_text);
    let mut result = String::new();
    let now = current_time_hms();

    for ch in text_str.chars() {
        if *at_line_start && ch != '\r' && ch != '\n' {
            result.push_str(&now);
            *at_line_start = false;
        }
        result.push(ch);
        if ch == '\n' {
            *at_line_start = true;
        }
    }
    result.into_bytes()
}

/// 运行异步串口会话。
pub async fn run_serial_session(
    terminal_id: String,
    config: SerialConfig,
    event_tx: Sender<PtyEvent>,
    mut input_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    exit_signal: Arc<AtomicBool>,
) -> Result<()> {
    let charset = config.charset.as_deref().unwrap_or(velowork_core::charset::DEFAULT_CHARSET);
    let auto_reconnect = config.auto_reconnect;

    if config.port.trim().is_empty() {
        let msg = "\r\n\x1b[31m[Failed to open serial port: No port specified. Please configure a serial port in session settings]\x1b[0m\r\n";
        let _ = event_tx.send(PtyEvent::Data {
            terminal_id: terminal_id.clone(),
            data: msg.as_bytes().to_vec(),
        }).await;
        let _ = event_tx.send(PtyEvent::Exit {
            terminal_id,
            exit_code: Some(1),
        }).await;
        return Err(anyhow::anyhow!("No serial port specified"));
    }

    'reconnect_loop: loop {
        if exit_signal.load(Ordering::Relaxed) {
            break 'reconnect_loop;
        }

        let builder = tokio_serial::new(&config.port, config.baud_rate)
            .data_bits(config.resolved_data_bits())
            .stop_bits(config.resolved_stop_bits())
            .parity(config.resolved_parity())
            .flow_control(config.resolved_flow_control())
            .timeout(Duration::from_millis(100));

        let mut stream = match builder.open_native_async() {
            Ok(s) => s,
            Err(e) => {
                if !auto_reconnect {
                    let msg = format!("\r\n\x1b[31m[Failed to open serial port '{}': {}]\x1b[0m\r\n", config.port, e);
                    let _ = event_tx.send(PtyEvent::Data {
                        terminal_id: terminal_id.clone(),
                        data: msg.into_bytes(),
                    }).await;
                    let _ = event_tx.send(PtyEvent::Exit {
                        terminal_id,
                        exit_code: Some(1),
                    }).await;
                    return Err(anyhow::anyhow!("Failed to open serial port '{}': {:#}", config.port, e));
                }
                let msg = format!("\x1b[33m[Failed to open serial port '{}': {}. Retrying in 1s...]\x1b[0m\r\n", config.port, e);
                let _ = event_tx.send(PtyEvent::Data { terminal_id: terminal_id.clone(), data: msg.into_bytes() }).await;
                tokio::time::sleep(Duration::from_millis(1000)).await;
                continue 'reconnect_loop;
            }
        };

        // 设置初始 DTR / RTS 引脚电平
        let _ = stream.write_data_terminal_ready(config.dtr);
        let _ = stream.write_request_to_send(config.rts);

        let (mut reader, mut writer) = tokio::io::split(stream);
        let mut read_buf = [0u8; 4096];
        let mut at_line_start = true;

        loop {
            if exit_signal.load(Ordering::Relaxed) {
                break 'reconnect_loop;
            }

            tokio::select! {
                // 从串口读取数据并发送至终端事件通道
                read_res = reader.read(&mut read_buf) => {
                    match read_res {
                        Ok(0) => {
                            log::info!("[terminal:serial] Serial port closed | port={}", config.port);
                            break;
                        }
                        Ok(n) => {
                            let processed = format_serial_output_data(
                                &read_buf[..n],
                                charset,
                                &config.display_mode,
                                config.timestamps,
                                &mut at_line_start,
                            );
                            if event_tx.send(PtyEvent::Data { terminal_id: terminal_id.clone(), data: processed }).await.is_err() {
                                break 'reconnect_loop;
                            }
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                            // 正常超时，继续监听
                        }
                        Err(e) => {
                            log::warn!("[terminal:serial] Serial read error | port={} | error: {:#}", config.port, e);
                            break;
                        }
                    }
                }

                // 从终端接收键盘输入并写入串口
                input_opt = input_rx.recv() => {
                    match input_opt {
                        Some(data) => {
                            let formatted = format_serial_input_line_ending(&data, &config.line_ending);
                            let target_bytes = crate::pty_manager::transcode_from_utf8(&formatted, charset);
                            if let Err(e) = writer.write_all(&target_bytes).await {
                                log::warn!("[terminal:serial] Serial write error | port={} | error: {:#}", config.port, e);
                                break;
                            }
                            let _ = writer.flush().await;

                            if config.local_echo {
                                let mut echo_data = Vec::new();
                                for &b in &data {
                                    if b == b'\r' || b == b'\n' {
                                        echo_data.extend_from_slice(b"\r\n");
                                    } else {
                                        echo_data.push(b);
                                    }
                                }
                                let _ = event_tx.send(PtyEvent::Data {
                                    terminal_id: terminal_id.clone(),
                                    data: echo_data,
                                }).await;
                            }
                        }
                        None => {
                            break 'reconnect_loop;
                        }
                    }
                }
            }
        }

        if !auto_reconnect || exit_signal.load(Ordering::Relaxed) {
            break 'reconnect_loop;
        }

        let msg = format!("\x1b[33m[Serial port '{}' disconnected. Reconnecting in 1s...]\x1b[0m\r\n", config.port);
        let _ = event_tx.send(PtyEvent::Data { terminal_id: terminal_id.clone(), data: msg.into_bytes() }).await;
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }

    let _ = event_tx.send(PtyEvent::Exit { terminal_id, exit_code: None }).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_serial_input_line_ending() {
        let input = b"hello\rworld\r\nfoo\nbar";

        let crlf = format_serial_input_line_ending(input, "crlf");
        assert_eq!(crlf, b"hello\r\nworld\r\nfoo\r\nbar");

        let lf = format_serial_input_line_ending(input, "lf");
        assert_eq!(lf, b"hello\nworld\nfoo\nbar");

        let cr = format_serial_input_line_ending(input, "cr");
        assert_eq!(cr, b"hello\rworld\rfoo\rbar");
    }

    #[test]
    fn test_format_serial_output_data_hex() {
        let raw = [0x01, 0x0A, 0xFF];
        let mut at_line_start = true;
        let out = format_serial_output_data(&raw, "utf-8", "hex", false, &mut at_line_start);
        assert_eq!(String::from_utf8_lossy(&out), "01 0A FF ");
    }

    #[test]
    fn test_format_serial_output_data_timestamps() {
        let raw1 = b"first line\nsecond ";
        let raw2 = b"line\n";
        let mut at_line_start = true;

        let out1 = format_serial_output_data(raw1, "utf-8", "text", true, &mut at_line_start);
        let s1 = String::from_utf8_lossy(&out1);
        assert!(s1.starts_with('['));
        assert!(s1.contains("first line\n"));
        assert!(!at_line_start);

        let out2 = format_serial_output_data(raw2, "utf-8", "text", true, &mut at_line_start);
        let s2 = String::from_utf8_lossy(&out2);
        // raw2 should NOT start with timestamp because at_line_start was false
        assert!(!s2.starts_with('['));
        assert_eq!(s2, "line\n");
        assert!(at_line_start);
    }

    #[tokio::test]
    async fn test_serial_session_empty_port_error() {
        let (event_tx, event_rx) = async_channel::unbounded();
        let (_input_tx, input_rx) = tokio::sync::mpsc::channel(1);
        let exit_signal = Arc::new(AtomicBool::new(false));

        let config = SerialConfig {
            port: "".to_string(),
            ..Default::default()
        };

        let res = run_serial_session("term-1".to_string(), config, event_tx, input_rx, exit_signal).await;
        assert!(res.is_err());

        let ev1 = event_rx.recv().await.unwrap();
        match ev1 {
            PtyEvent::Data { terminal_id, data } => {
                assert_eq!(terminal_id, "term-1");
                let text = String::from_utf8_lossy(&data);
                assert!(text.contains("No port specified"));
            }
            _ => panic!("Expected PtyEvent::Data"),
        }

        let ev2 = event_rx.recv().await.unwrap();
        match ev2 {
            PtyEvent::Exit { terminal_id, exit_code } => {
                assert_eq!(terminal_id, "term-1");
                assert_eq!(exit_code, Some(1));
            }
            _ => panic!("Expected PtyEvent::Exit"),
        }
    }

    #[tokio::test]
    async fn test_serial_session_nonexistent_port_error() {
        let (event_tx, event_rx) = async_channel::unbounded();
        let (_input_tx, input_rx) = tokio::sync::mpsc::channel(1);
        let exit_signal = Arc::new(AtomicBool::new(false));

        let config = SerialConfig {
            port: "COM_VELOWORK_NONEXISTENT_99999".to_string(),
            auto_reconnect: false,
            ..Default::default()
        };

        let res = run_serial_session("term-2".to_string(), config, event_tx, input_rx, exit_signal).await;
        assert!(res.is_err());

        let ev1 = event_rx.recv().await.unwrap();
        match ev1 {
            PtyEvent::Data { terminal_id, data } => {
                assert_eq!(terminal_id, "term-2");
                let text = String::from_utf8_lossy(&data);
                assert!(text.contains("Failed to open serial port 'COM_VELOWORK_NONEXISTENT_99999'"));
            }
            _ => panic!("Expected PtyEvent::Data"),
        }

        let ev2 = event_rx.recv().await.unwrap();
        match ev2 {
            PtyEvent::Exit { terminal_id, exit_code } => {
                assert_eq!(terminal_id, "term-2");
                assert_eq!(exit_code, Some(1));
            }
            _ => panic!("Expected PtyEvent::Exit"),
        }
    }
}
