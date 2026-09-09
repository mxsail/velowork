//! ZMODEM protocol constants, framing, CRC16/CRC32 calculation, and header detection.

pub const ZPAD: u8 = b'*';
pub const ZDLE: u8 = 0x18;
pub const ZDLEE: u8 = 0x18 ^ 0x40; // 0x58
pub const ZBIN: u8 = b'A';
pub const ZHEX: u8 = b'B';
pub const ZBIN32: u8 = b'C';

// ZMODEM Header types
pub const ZRINIT: u8 = 0;
pub const ZRQINIT: u8 = 1;
pub const ZFILE: u8 = 2;
pub const ZNAK: u8 = 3;
pub const ZACK: u8 = 4;
pub const ZFIN: u8 = 6;
pub const ZRPOS: u8 = 9;
pub const ZDATA: u8 = 10;
pub const ZEOF: u8 = 11;
pub const ZFERR: u8 = 12;
pub const ZCRC: u8 = 13;
pub const ZCHALLENGE: u8 = 14;
pub const ZCOMPL: u8 = 15;
pub const ZCAN: u8 = 16;
pub const ZFREECNT: u8 = 17;
pub const ZCOMMAND: u8 = 18;
pub const ZSTDERR: u8 = 19;

// Frame end delimiters
pub const ZCRCE: u8 = b'h'; // 0x68: Data subpacket end, next is frame header
pub const ZCRCG: u8 = b'i'; // 0x69: Data subpacket continues, next is another subpacket
pub const ZCRCQ: u8 = b'j'; // 0x6a: Data subpacket continues, sender requests ZACK
pub const ZCRCW: u8 = b'k'; // 0x6b: Data subpacket end, sender waits for ZACK

// ZRINIT capability flags
pub const CANFDX: u8 = 0x01; // Can do full duplex
pub const CANOVIO: u8 = 0x02; // Can do overlap I/O
pub const CANBRK: u8 = 0x04; // Can send break signal
pub const CANCRY: u8 = 0x08; // Can encrypt
pub const CANLZW: u8 = 0x10; // Can compress
pub const CANFC32: u8 = 0x20; // Can use 32-bit CRC
pub const ESCCTL: u8 = 0x40; // Expects control characters escaped
pub const ESC8: u8 = 0x80; // Expects 8th bit escaped

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZmodemHeaderType {
    Zrinit,
    Zrqinit,
    Zfile,
    Znak,
    Zack,
    Zfin,
    Zrpos(u32),
    Zdata(u32),
    Zeof,
    Zcan,
    Unknown(u8),
}

impl ZmodemHeaderType {
    pub fn from_u8(t: u8, flags: [u8; 4]) -> Self {
        let pos = u32::from_le_bytes(flags);
        match t {
            ZRINIT => Self::Zrinit,
            ZRQINIT => Self::Zrqinit,
            ZFILE => Self::Zfile,
            ZNAK => Self::Znak,
            ZACK => Self::Zack,
            ZFIN => Self::Zfin,
            ZRPOS => Self::Zrpos(pos),
            ZDATA => Self::Zdata(pos),
            ZEOF => Self::Zeof,
            ZCAN => Self::Zcan,
            other => Self::Unknown(other),
        }
    }
}

/// Calculate CRC16-CCITT (polynomial 0x1021, init 0)
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Calculate CRC32 (IEEE 802.3 polynomial 0xEDB88320, init 0xFFFFFFFF, inverted)
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFFFFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = if (crc & 1) != 0 { 0xEDB88320 } else { 0 };
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

/// Format a ZMODEM hex header.
/// Structure: `**\x18B<2 hex type><8 hex flags><4 hex crc16>\r\n\x11`
pub fn build_hex_header(header_type: u8, flags: [u8; 4]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(5);
    payload.push(header_type);
    payload.extend_from_slice(&flags);

    let crc = crc16(&payload);
    let mut hex = String::with_capacity(14);
    hex.push_str(&format!("{:02x}", header_type));
    for &f in &flags {
        hex.push_str(&format!("{:02x}", f));
    }
    hex.push_str(&format!("{:04x}", crc));

    let mut out = Vec::with_capacity(20);
    out.push(ZPAD);
    out.push(ZPAD);
    out.push(ZDLE);
    out.push(ZHEX);
    out.extend_from_slice(hex.as_bytes());
    out.push(b'\r');
    out.push(b'\n');
    out.push(0x11); // XON
    out
}

/// Parse a ZMODEM hex header from bytes.
pub fn parse_hex_header(data: &[u8]) -> Option<(u8, [u8; 4], usize)> {
    // Look for ZPAD ZPAD ZDLE ZHEX ("**\x18B")
    let pos = data.windows(4).position(|w| w == [ZPAD, ZPAD, ZDLE, ZHEX])?;
    let hex_start = pos + 4;
    if data.len() < hex_start + 14 {
        return None;
    }

    let hex_str = std::str::from_utf8(&data[hex_start..hex_start + 14]).ok()?;
    let header_type = u8::from_str_radix(&hex_str[0..2], 16).ok()?;

    let mut flags = [0u8; 4];
    for i in 0..4 {
        flags[i] = u8::from_str_radix(&hex_str[2 + i * 2..4 + i * 2], 16).ok()?;
    }

    let crc_given = u16::from_str_radix(&hex_str[10..14], 16).ok()?;
    let mut payload = Vec::with_capacity(5);
    payload.push(header_type);
    payload.extend_from_slice(&flags);

    if crc16(&payload) == crc_given {
        let consumed = hex_start + 14;
        // Optionally consume trailing \r\n\x11
        let mut end = consumed;
        while end < data.len() && (data[end] == b'\r' || data[end] == b'\n' || data[end] == 0x11 || data[end] == 0x13) {
            end += 1;
        }
        Some((header_type, flags, end))
    } else {
        None
    }
}

/// ZDLE escape bytes in buffer for transmission
pub fn zdle_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 2);
    for &b in data {
        match b {
            0x10 | 0x11 | 0x13 | 0x18 | 0x7f | 0x8d | 0x90 | 0x91 | 0x93 => {
                out.push(ZDLE);
                out.push(b ^ 0x40);
            }
            _ => out.push(b),
        }
    }
    out
}

/// ZDLE decode incoming bytes, unescaping ZDLE + (b ^ 0x40).
/// Returns unescaped bytes and any unconsumed slice.
pub fn zdle_decode(data: &[u8]) -> (Vec<u8>, usize) {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        if b == ZDLE {
            if i + 1 < data.len() {
                let next = data[i + 1];
                if next == ZCRCE || next == ZCRCG || next == ZCRCQ || next == ZCRCW {
                    // Frame delimiter reached
                    return (out, i);
                }
                out.push(next ^ 0x40);
                i += 2;
            } else {
                // Incomplete escape at buffer end
                break;
            }
        } else {
            out.push(b);
            i += 1;
        }
    }
    (out, i)
}

/// Detector for ZMODEM triggers in incoming terminal PTY output stream.
#[derive(Default)]
pub struct ZmodemDetector {
    buffer: Vec<u8>,
}

impl ZmodemDetector {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(1024),
        }
    }

    /// Feed incoming bytes and detect if a ZMODEM header (`ZRINIT` or `ZFILE` or `ZRQINIT`) is present.
    pub fn inspect(&mut self, bytes: &[u8]) -> Option<ZmodemHeaderType> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > 4096 {
            let drain = self.buffer.len() - 2048;
            self.buffer.drain(..drain);
        }

        // Check for cancel sequence: 5x CAN (0x18)
        if self.buffer.windows(5).any(|w| w == [ZDLE, ZDLE, ZDLE, ZDLE, ZDLE]) {
            self.buffer.clear();
            return Some(ZmodemHeaderType::Zcan);
        }

        if let Some((htype, flags, consumed)) = parse_hex_header(&self.buffer) {
            let parsed = ZmodemHeaderType::from_u8(htype, flags);
            self.buffer.drain(..consumed);
            return Some(parsed);
        }

        None
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc16_and_hex_header() {
        let flags = [0x01, 0x02, 0x03, 0x04];
        let header = build_hex_header(ZRINIT, flags);
        assert!(header.starts_with(b"**\x18B"));

        let (htype, parsed_flags, consumed) = parse_hex_header(&header).expect("valid hex header");
        assert_eq!(htype, ZRINIT);
        assert_eq!(parsed_flags, flags);
        assert!(consumed <= header.len());
    }

    #[test]
    fn test_crc32() {
        let data = b"123456789";
        // Standard CRC-32 for "123456789" is 0xCBF43926
        assert_eq!(crc32(data), 0xCBF43926);
    }

    #[test]
    fn test_zdle_encode_and_decode() {
        let raw = vec![0x00, 0x11, 0x18, 0x41, 0x7f, 0x93];
        let encoded = zdle_encode(&raw);
        assert!(encoded.len() > raw.len());

        let (decoded, consumed) = zdle_decode(&encoded);
        assert_eq!(decoded, raw);
        assert_eq!(consumed, encoded.len());
    }

    #[test]
    fn test_zmodem_detector() {
        let mut detector = ZmodemDetector::new();
        assert_eq!(detector.inspect(b"hello world\r\n"), None);

        let rz_header = build_hex_header(ZRINIT, [0, 0, 0, 0]);
        let res = detector.inspect(&rz_header);
        assert_eq!(res, Some(ZmodemHeaderType::Zrinit));

        let sz_header = build_hex_header(ZFILE, [0, 0, 0, 0]);
        let res2 = detector.inspect(&sz_header);
        assert_eq!(res2, Some(ZmodemHeaderType::Zfile));
    }
}
