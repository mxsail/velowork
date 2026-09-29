//! ZMODEM protocol constants, framing, CRC16/CRC32 calculation, and header detection.
//!
//! Compliant with Chuck Forsberg's ZMODEM protocol specification and lrzsz.

pub const ZPAD: u8 = b'*';
pub const ZDLE: u8 = 0x18;
pub const ZDLEE: u8 = 0x18 ^ 0x40; // 0x58 ('X')
pub const ZBIN: u8 = b'A';
pub const ZHEX: u8 = b'B';
pub const ZBIN32: u8 = b'C';

// ZMODEM Header types (Standard RFC / lrzsz definitions)
pub const ZRQINIT: u8 = 0;   // Request receive init (sent by sz)
pub const ZRINIT: u8 = 1;    // Receive init (sent by rz)
pub const ZSINIT: u8 = 2;    // Send init sequence (optional)
pub const ZACK: u8 = 3;      // ACK to frame
pub const ZFILE: u8 = 4;     // File name and metadata
pub const ZSKIP: u8 = 5;     // Skip this file
pub const ZNAK: u8 = 6;      // Last packet was garbled
pub const ZABORT: u8 = 7;    // Abort batch transfers
pub const ZFIN: u8 = 8;      // Finish session
pub const ZRPOS: u8 = 9;     // Resume data transfer at position
pub const ZDATA: u8 = 10;    // Data packet(s) follow
pub const ZEOF: u8 = 11;     // End of file
pub const ZFERR: u8 = 12;    // Fatal read/write error
pub const ZCRC: u8 = 13;     // Request for file CRC and response
pub const ZCHALLENGE: u8 = 14;// Receiver's challenge
pub const ZCOMPL: u8 = 15;   // Request is complete
pub const ZCAN: u8 = 16;     // Other end cancelled with 5x CAN
pub const ZFREECNT: u8 = 17; // Request for free bytes on filesystem
pub const ZCOMMAND: u8 = 18; // Command from sending program
pub const ZSTDERR: u8 = 19;  // Output destined for stderr

// Frame end delimiters for data subpackets
pub const ZCRCE: u8 = b'h'; // 0x68: Data subpacket end, header follows, no ACK
pub const ZCRCG: u8 = b'i'; // 0x69: Data subpacket continues, next is another subpacket
pub const ZCRCQ: u8 = b'j'; // 0x6a: Data subpacket continues, receiver must reply with ZACK
pub const ZCRCW: u8 = b'k'; // 0x6b: Data subpacket end, sender waits for ZACK

// ZRUB constants for 0x7f and 0xff encoding per ZMODEM specification
pub const ZRUB0: u8 = b'l'; // 0x6c: 0177 (0x7F / DEL) encoded with ZDLE
pub const ZRUB1: u8 = b'm'; // 0x6d: 0377 (0xFF) encoded with ZDLE

/// Helper: decode a byte escaped with ZDLE.
/// According to Chuck Forsberg's ZMODEM spec and lrzsz:
/// - ZDLE + ZRUB0 ('l') decodes to 0x7F (0177 DEL)
/// - ZDLE + ZRUB1 ('m') decodes to 0xFF (0377)
/// - ZDLE + c decodes to c ^ 0x40
#[inline]
pub fn zdle_unescape_byte(b: u8) -> u8 {
    match b {
        ZRUB0 => 0x7f,
        ZRUB1 => 0xff,
        other => other ^ 0x40,
    }
}

// ZRINIT capability flags
pub const CANFDX: u8 = 0x01; // Can do full duplex
pub const CANOVIO: u8 = 0x02; // Can do overlap I/O
pub const CANBRK: u8 = 0x04; // Can send break signal
pub const CANCRY: u8 = 0x08; // Can encrypt
pub const CANLZW: u8 = 0x10; // Can compress
pub const CANFC32: u8 = 0x20; // Can use 32-bit CRC
pub const ESCCTL: u8 = 0x40; // Expects control characters escaped
pub const ESC8: u8 = 0x80;   // Expects 8th bit escaped

// ZFILE flags and header array byte offsets [ZF3, ZF2, ZF1, ZF0]
pub const ZF3_IDX: usize = 0;
pub const ZF2_IDX: usize = 1;
pub const ZF1_IDX: usize = 2;
pub const ZF0_IDX: usize = 3;

pub const ZCBIN: u8 = 1;      // Binary transfer - no conversion (ZF0)
pub const ZF1_ZMCLOB: u8 = 4; // Replace/overwrite existing destination file (ZF1)
pub const ZF1_ZMPROT: u8 = 6; // Protect destination file: skip if exists or write-protected (ZF1)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZmodemHeaderFormat {
    Hex,
    Binary16,
    Binary32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZmodemHeaderType {
    Zrqinit,
    Zrinit,
    Zsinit,
    Zack,
    Zfile,
    Zskip,
    Znak,
    Zabort,
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
            ZRQINIT => Self::Zrqinit,
            ZRINIT => Self::Zrinit,
            ZSINIT => Self::Zsinit,
            ZACK => Self::Zack,
            ZFILE => Self::Zfile,
            ZSKIP => Self::Zskip,
            ZNAK => Self::Znak,
            ZABORT => Self::Zabort,
            ZFIN => Self::Zfin,
            ZRPOS => Self::Zrpos(pos),
            ZDATA => Self::Zdata(pos),
            ZEOF => Self::Zeof,
            ZCAN => Self::Zcan,
            other => Self::Unknown(other),
        }
    }

    pub fn to_u8(&self) -> u8 {
        match self {
            Self::Zrqinit => ZRQINIT,
            Self::Zrinit => ZRINIT,
            Self::Zsinit => ZSINIT,
            Self::Zack => ZACK,
            Self::Zfile => ZFILE,
            Self::Zskip => ZSKIP,
            Self::Znak => ZNAK,
            Self::Zabort => ZABORT,
            Self::Zfin => ZFIN,
            Self::Zrpos(_) => ZRPOS,
            Self::Zdata(_) => ZDATA,
            Self::Zeof => ZEOF,
            Self::Zcan => ZCAN,
            Self::Unknown(o) => *o,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZmodemHeader {
    pub header_type: u8,
    pub flags: [u8; 4],
    pub format: ZmodemHeaderFormat,
}

impl ZmodemHeader {
    pub fn parsed_type(&self) -> ZmodemHeaderType {
        ZmodemHeaderType::from_u8(self.header_type, self.flags)
    }

    pub fn position(&self) -> u32 {
        u32::from_le_bytes(self.flags)
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

/// Format a ZMODEM binary header with 16-bit CRC (`*\x18A...`)
pub fn build_binary16_header_with_escctl(header_type: u8, flags: [u8; 4], escctl: bool) -> Vec<u8> {
    let mut payload = Vec::with_capacity(5);
    payload.push(header_type);
    payload.extend_from_slice(&flags);

    let crc = crc16(&payload);
    let mut out = Vec::with_capacity(16);
    out.push(ZPAD);
    out.push(ZDLE);
    out.push(ZBIN);

    out.extend_from_slice(&zdle_encode_with_escctl(&payload, escctl));
    let crc_bytes = crc.to_be_bytes();
    out.extend_from_slice(&zdle_encode_with_escctl(&crc_bytes, escctl));
    out
}

/// Format a ZMODEM binary header with 16-bit CRC (`*\x18A...`) (non-ESCCTL)
pub fn build_binary16_header(header_type: u8, flags: [u8; 4]) -> Vec<u8> {
    build_binary16_header_with_escctl(header_type, flags, false)
}

/// Format a ZMODEM binary header with 32-bit CRC (`*\x18C...`)
pub fn build_binary32_header_with_escctl(header_type: u8, flags: [u8; 4], escctl: bool) -> Vec<u8> {
    let mut payload = Vec::with_capacity(5);
    payload.push(header_type);
    payload.extend_from_slice(&flags);

    let crc = crc32(&payload);
    let mut out = Vec::with_capacity(20);
    out.push(ZPAD);
    out.push(ZDLE);
    out.push(ZBIN32);

    out.extend_from_slice(&zdle_encode_with_escctl(&payload, escctl));
    let crc_bytes = crc.to_le_bytes();
    out.extend_from_slice(&zdle_encode_with_escctl(&crc_bytes, escctl));
    out
}

/// Format a ZMODEM binary header with 32-bit CRC (`*\x18C...`) (non-ESCCTL)
pub fn build_binary32_header(header_type: u8, flags: [u8; 4]) -> Vec<u8> {
    build_binary32_header_with_escctl(header_type, flags, false)
}

/// Parse a ZMODEM hex header from bytes.
/// Returns (header_type, flags, start_pos, end_pos) if found.
pub fn parse_hex_header(data: &[u8]) -> Option<(u8, [u8; 4], usize, usize)> {
    let pos = data.windows(4).position(|w| {
        w[0] == ZPAD && w[1] == ZPAD && w[2] == ZDLE && (w[3] == ZHEX || w[3] == b'b')
    })?;
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
        let mut end = consumed;
        while end < data.len()
            && (data[end] == b'\r' || data[end] == b'\n' || data[end] == 0x11 || data[end] == 0x13)
        {
            end += 1;
        }
        Some((header_type, flags, pos, end))
    } else {
        None
    }
}

/// Helper: unescape up to `count` bytes from raw data starting at `offset`.
/// Returns unescaped bytes and total consumed bytes from raw slice.
fn unescape_n_bytes(raw: &[u8], count: usize) -> Option<(Vec<u8>, usize)> {
    let mut out = Vec::with_capacity(count);
    let mut i = 0;
    while i < raw.len() && out.len() < count {
        let b = raw[i];
        if b == ZDLE {
            if i + 1 >= raw.len() {
                return None; // Incomplete escape sequence
            }
            let next = raw[i + 1];
            out.push(zdle_unescape_byte(next));
            i += 2;
        } else {
            out.push(b);
            i += 1;
        }
    }
    if out.len() == count {
        Some((out, i))
    } else {
        None
    }
}

/// Parse a ZMODEM binary 16-bit CRC header (`*\x18A...`).
/// Returns (header_type, flags, start_pos, end_pos) if found.
pub fn parse_binary16_header(data: &[u8]) -> Option<(u8, [u8; 4], usize, usize)> {
    let pos = data
        .windows(3)
        .position(|w| w[0] == ZPAD && w[1] == ZDLE && (w[2] == ZBIN || w[2] == b'a'))?;
    let start = pos + 3;
    let (payload, consumed_payload) = unescape_n_bytes(&data[start..], 5)?;
    let (crc_bytes, consumed_crc) =
        unescape_n_bytes(&data[start + consumed_payload..], 2)?;

    let header_type = payload[0];
    let mut flags = [0u8; 4];
    flags.copy_from_slice(&payload[1..5]);

    let crc_given = u16::from_be_bytes([crc_bytes[0], crc_bytes[1]]);
    if crc16(&payload) == crc_given {
        Some((header_type, flags, pos, start + consumed_payload + consumed_crc))
    } else {
        None
    }
}

/// Parse a ZMODEM binary 32-bit CRC header (`*\x18C...`).
/// Returns (header_type, flags, start_pos, end_pos) if found.
pub fn parse_binary32_header(data: &[u8]) -> Option<(u8, [u8; 4], usize, usize)> {
    let pos = data
        .windows(3)
        .position(|w| w[0] == ZPAD && w[1] == ZDLE && (w[2] == ZBIN32 || w[2] == b'c'))?;
    let start = pos + 3;
    let (payload, consumed_payload) = unescape_n_bytes(&data[start..], 5)?;
    let (crc_bytes, consumed_crc) =
        unescape_n_bytes(&data[start + consumed_payload..], 4)?;

    let header_type = payload[0];
    let mut flags = [0u8; 4];
    flags.copy_from_slice(&payload[1..5]);

    let crc_given = u32::from_le_bytes([crc_bytes[0], crc_bytes[1], crc_bytes[2], crc_bytes[3]]);
    if crc32(&payload) == crc_given {
        Some((header_type, flags, pos, start + consumed_payload + consumed_crc))
    } else {
        None
    }
}

/// Parse any ZMODEM header (Hex, Binary16, or Binary32).
/// Returns (Header, start_pos, end_pos) for the earliest valid header found in buffer.
pub fn parse_any_header(data: &[u8]) -> Option<(ZmodemHeader, usize, usize)> {
    let mut candidates = Vec::new();

    if let Some((t, f, s, e)) = parse_hex_header(data) {
        candidates.push((
            s,
            e,
            ZmodemHeader {
                header_type: t,
                flags: f,
                format: ZmodemHeaderFormat::Hex,
            },
        ));
    }

    if let Some((t, f, s, e)) = parse_binary32_header(data) {
        candidates.push((
            s,
            e,
            ZmodemHeader {
                header_type: t,
                flags: f,
                format: ZmodemHeaderFormat::Binary32,
            },
        ));
    }

    if let Some((t, f, s, e)) = parse_binary16_header(data) {
        candidates.push((
            s,
            e,
            ZmodemHeader {
                header_type: t,
                flags: f,
                format: ZmodemHeaderFormat::Binary16,
            },
        ));
    }

    // Pick candidate with earliest start_pos
    candidates.into_iter().min_by_key(|c| c.0).map(|(s, e, h)| (h, s, e))
}

/// ZDLE escape bytes in buffer for transmission with optional ESCCTL mode.
/// If `escctl` is true, all ASCII control characters (0x00..=0x1F, 0x80..=0x9F) as well as
/// 0x7F, 0xFF, and ZDLE are escaped.
pub fn zdle_encode_with_escctl(data: &[u8], escctl: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 2);
    for &b in data {
        match b {
            0x7f => {
                out.push(ZDLE);
                out.push(ZRUB0);
            }
            0xff => {
                out.push(ZDLE);
                out.push(ZRUB1);
            }
            _ if escctl && (b & 0x60) == 0 => {
                out.push(ZDLE);
                out.push(b ^ 0x40);
            }
            0x10 | 0x11 | 0x13 | 0x18 | 0x8d | 0x90 | 0x91 | 0x93 => {
                out.push(ZDLE);
                out.push(b ^ 0x40);
            }
            _ => out.push(b),
        }
    }
    out
}

/// ZDLE escape bytes in buffer for transmission (standard non-ESCCTL mode)
pub fn zdle_encode(data: &[u8]) -> Vec<u8> {
    zdle_encode_with_escctl(data, false)
}

/// ZDLE decode incoming bytes, unescaping ZDLE sequences.
/// Returns unescaped bytes and total consumed raw slice.
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
                out.push(zdle_unescape_byte(next));
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

/// Encode a ZMODEM data subpacket with ESCCTL support.
/// Format: `[zdle_encode(data)] [ZDLE] [frame_end] [zdle_encode(crc)]`
pub fn encode_subpacket_with_escctl(data: &[u8], frame_end: u8, use_crc32: bool, escctl: bool) -> Vec<u8> {
    let encoded_data = zdle_encode_with_escctl(data, escctl);
    let mut out = Vec::with_capacity(encoded_data.len() + 12);
    out.extend_from_slice(&encoded_data);
    out.push(ZDLE);
    out.push(frame_end);

    if use_crc32 {
        let mut crc_input = Vec::with_capacity(data.len() + 1);
        crc_input.extend_from_slice(data);
        crc_input.push(frame_end);
        let crc = crc32(&crc_input);
        let crc_bytes = crc.to_le_bytes();
        out.extend_from_slice(&zdle_encode_with_escctl(&crc_bytes, escctl));
    } else {
        let mut crc_input = Vec::with_capacity(data.len() + 1);
        crc_input.extend_from_slice(data);
        crc_input.push(frame_end);
        let crc = crc16(&crc_input);
        let crc_bytes = crc.to_be_bytes();
        out.extend_from_slice(&zdle_encode_with_escctl(&crc_bytes, escctl));
    }

    out
}

/// Encode a ZMODEM data subpacket (standard non-ESCCTL mode).
/// Format: `[zdle_encode(data)] [ZDLE] [frame_end] [zdle_encode(crc)]`
pub fn encode_subpacket(data: &[u8], frame_end: u8, use_crc32: bool) -> Vec<u8> {
    encode_subpacket_with_escctl(data, frame_end, use_crc32, false)
}

/// Decode a ZMODEM data subpacket from incoming buffer.
/// Returns (payload, frame_end, bytes_consumed) if a complete and valid subpacket is found.
pub fn decode_subpacket(data: &[u8], use_crc32: bool) -> Option<(Vec<u8>, u8, usize)> {
    let mut payload = Vec::with_capacity(data.len());
    let mut i = 0;
    let mut frame_end = None;

    while i < data.len() {
        let b = data[i];
        if b == ZDLE {
            if i + 1 >= data.len() {
                return None; // Wait for more data
            }
            let next = data[i + 1];
            if next == ZCRCE || next == ZCRCG || next == ZCRCQ || next == ZCRCW {
                frame_end = Some(next);
                i += 2;
                break;
            } else {
                payload.push(zdle_unescape_byte(next));
                i += 2;
            }
        } else {
            payload.push(b);
            i += 1;
        }
    }

    let end_delim = frame_end?;
    let crc_len = if use_crc32 { 4 } else { 2 };
    let (crc_bytes, crc_consumed) = unescape_n_bytes(&data[i..], crc_len)?;
    i += crc_consumed;

    if use_crc32 {
        let mut crc_input = Vec::with_capacity(payload.len() + 1);
        crc_input.extend_from_slice(&payload);
        crc_input.push(end_delim);
        let crc_expected = crc32(&crc_input);
        let crc_given = u32::from_le_bytes([crc_bytes[0], crc_bytes[1], crc_bytes[2], crc_bytes[3]]);
        if crc_expected == crc_given {
            Some((payload, end_delim, i))
        } else {
            None
        }
    } else {
        let mut crc_input = Vec::with_capacity(payload.len() + 1);
        crc_input.extend_from_slice(&payload);
        crc_input.push(end_delim);
        let crc_expected = crc16(&crc_input);
        let crc_given = u16::from_be_bytes([crc_bytes[0], crc_bytes[1]]);
        if crc_expected == crc_given {
            Some((payload, end_delim, i))
        } else {
            None
        }
    }
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

    /// Feed incoming bytes and detect if a ZMODEM header is present.
    /// Returns Some((HeaderType, start_offset_in_incoming_chunk, header_len)) if a header is detected.
    pub fn inspect(&mut self, bytes: &[u8]) -> Option<(ZmodemHeaderType, usize, usize)> {
        let prev_len = self.buffer.len();
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > 4096 {
            let drain = self.buffer.len() - 2048;
            self.buffer.drain(..drain);
        }

        // Check for cancel sequence: 5x CAN (0x18)
        if let Some(can_pos) = self.buffer.windows(5).position(|w| w == [ZDLE, ZDLE, ZDLE, ZDLE, ZDLE]) {
            self.buffer.clear();
            let relative_pos = can_pos.saturating_sub(prev_len);
            return Some((ZmodemHeaderType::Zcan, relative_pos, 5));
        }

        if let Some((header, start_pos, consumed)) = parse_any_header(&self.buffer) {
            let ptype = header.parsed_type();
            let header_len = consumed.saturating_sub(start_pos);
            self.buffer.drain(..consumed);
            let relative_pos = start_pos.saturating_sub(prev_len);
            return Some((ptype, relative_pos, header_len));
        }

        None
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
    }
}

/// Represents a ZMODEM frame that was detected and stripped from a raw byte stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrippedFrame {
    pub header_type: ZmodemHeaderType,
    pub original_start: usize,
    pub original_len: usize,
}

/// Strip all ZMODEM protocol frames (Hex headers, Binary headers, 5x CAN) from a byte slice.
/// Returns `(clean_bytes, stripped_frames)`.
pub fn strip_all_zmodem_frames(data: &[u8]) -> (Vec<u8>, Vec<StrippedFrame>) {
    let mut clean = Vec::with_capacity(data.len());
    let mut frames = Vec::new();
    let mut cur = 0;

    while cur < data.len() {
        let remaining = &data[cur..];

        // 1. Check for cancel sequence: 5x CAN (0x18)
        let can_match = remaining.windows(5).position(|w| w == [ZDLE, ZDLE, ZDLE, ZDLE, ZDLE]);

        // 2. Check for any valid ZMODEM header
        let header_match = parse_any_header(remaining);

        match (can_match, header_match) {
            (Some(c_pos), Some((h, h_start, h_end))) => {
                if c_pos < h_start {
                    // CAN sequence came first
                    clean.extend_from_slice(&remaining[..c_pos]);
                    let mut can_end = c_pos + 5;
                    while can_end < remaining.len() && (remaining[can_end] == ZDLE || remaining[can_end] == 0x08) {
                        can_end += 1;
                    }
                    frames.push(StrippedFrame {
                        header_type: ZmodemHeaderType::Zcan,
                        original_start: cur + c_pos,
                        original_len: can_end - c_pos,
                    });
                    cur += can_end;
                } else {
                    // Header came first
                    clean.extend_from_slice(&remaining[..h_start]);
                    frames.push(StrippedFrame {
                        header_type: h.parsed_type(),
                        original_start: cur + h_start,
                        original_len: h_end - h_start,
                    });
                    cur += h_end;
                }
            }
            (Some(c_pos), None) => {
                clean.extend_from_slice(&remaining[..c_pos]);
                let mut can_end = c_pos + 5;
                while can_end < remaining.len() && (remaining[can_end] == ZDLE || remaining[can_end] == 0x08) {
                    can_end += 1;
                }
                frames.push(StrippedFrame {
                    header_type: ZmodemHeaderType::Zcan,
                    original_start: cur + c_pos,
                    original_len: can_end - c_pos,
                });
                cur += can_end;
            }
            (None, Some((h, h_start, h_end))) => {
                clean.extend_from_slice(&remaining[..h_start]);
                frames.push(StrippedFrame {
                    header_type: h.parsed_type(),
                    original_start: cur + h_start,
                    original_len: h_end - h_start,
                });
                cur += h_end;
            }
            (None, None) => {
                // No headers or cancel frames in remaining data
                clean.extend_from_slice(remaining);
                break;
            }
        }
    }

    (clean, frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc16_and_hex_header() {
        let flags = [0x01, 0x02, 0x03, 0x04];
        let header = build_hex_header(ZRINIT, flags);
        assert!(header.starts_with(b"**\x18B"));

        let (htype, parsed_flags, start_pos, end_pos) =
            parse_hex_header(&header).expect("valid hex header");
        assert_eq!(htype, ZRINIT);
        assert_eq!(parsed_flags, flags);
        assert_eq!(start_pos, 0);
        assert!(end_pos <= header.len());
    }

    #[test]
    fn test_binary16_header_roundtrip() {
        let flags = [0x10, 0x20, 0x30, 0x40];
        let raw = build_binary16_header(ZDATA, flags);
        assert!(raw.starts_with(b"*\x18A"));

        let (htype, parsed_flags, start_pos, end_pos) =
            parse_binary16_header(&raw).expect("valid binary16 header");
        assert_eq!(htype, ZDATA);
        assert_eq!(parsed_flags, flags);
        assert_eq!(start_pos, 0);
        assert_eq!(end_pos, raw.len());
    }

    #[test]
    fn test_binary32_header_roundtrip() {
        let flags = [0xaa, 0xbb, 0xcc, 0xdd];
        let raw = build_binary32_header(ZDATA, flags);
        assert!(raw.starts_with(b"*\x18C"));

        let (htype, parsed_flags, start_pos, end_pos) =
            parse_binary32_header(&raw).expect("valid binary32 header");
        assert_eq!(htype, ZDATA);
        assert_eq!(parsed_flags, flags);
        assert_eq!(start_pos, 0);
        assert_eq!(end_pos, raw.len());
    }

    #[test]
    fn test_subpacket_encode_decode_crc16() {
        let payload = b"Hello, ZMODEM CRC16 subpacket!";
        let encoded = encode_subpacket(payload, ZCRCW, false);
        let (decoded, frame_end, consumed) =
            decode_subpacket(&encoded, false).expect("decoded subpacket");
        assert_eq!(decoded, payload);
        assert_eq!(frame_end, ZCRCW);
        assert_eq!(consumed, encoded.len());
    }

    #[test]
    fn test_subpacket_encode_decode_crc32() {
        let payload = b"Hello, ZMODEM CRC32 high performance streaming!";
        let encoded = encode_subpacket(payload, ZCRCG, true);
        let (decoded, frame_end, consumed) =
            decode_subpacket(&encoded, true).expect("decoded subpacket");
        assert_eq!(decoded, payload);
        assert_eq!(frame_end, ZCRCG);
        assert_eq!(consumed, encoded.len());
    }

    #[test]
    fn test_crc16_against_lrzsz_vector() {
        // User's actual output from lrzsz rz: **B0100000023be50
        let payload = [0x01, 0x00, 0x00, 0x00, 0x23];
        assert_eq!(crc16(&payload), 0xbe50);

        let zrqinit_payload = [0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(crc16(&zrqinit_payload), 0x0000);
    }

    #[test]
    fn test_detector_with_prefix_text() {
        let mut detector = ZmodemDetector::new();
        let stream = b"rz waiting to receive.\r\n**\x18B0100000023be50\r\n\x11";
        let res = detector.inspect(stream);
        assert!(res.is_some());
        let (htype, offset, len) = res.unwrap();
        assert_eq!(htype, ZmodemHeaderType::Zrinit);
        // Prefix "rz waiting to receive.\r\n" is 24 bytes
        assert_eq!(offset, 24);
        assert!(len > 0);
    }

    #[test]
    fn test_detector_with_trailing_prompt() {
        let mut detector = ZmodemDetector::new();
        let stream = b"**\x18B0900000000a87c\r\nubuntu@host:~$ ";
        let res = detector.inspect(stream);
        assert!(res.is_some());
        let (htype, offset, len) = res.unwrap();
        assert_eq!(htype, ZmodemHeaderType::Zrpos(0));
        assert_eq!(offset, 0);
        assert_eq!(&stream[offset + len..], b"ubuntu@host:~$ ");
    }

    #[test]
    fn test_strip_all_zmodem_frames_multiple_packets() {
        let stream = b"rz waiting to receive.\r\n**\x18B0900000000a87c\r\n**\x18B0900000000a87c\r\n\x18\x18\x18\x18\x18\x08\x08\x08ubuntu@host:~$ ";
        let (clean, frames) = strip_all_zmodem_frames(stream);
        assert_eq!(clean, b"rz waiting to receive.\r\nubuntu@host:~$ ");
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].header_type, ZmodemHeaderType::Zrpos(0));
        assert_eq!(frames[1].header_type, ZmodemHeaderType::Zrpos(0));
        assert_eq!(frames[2].header_type, ZmodemHeaderType::Zcan);
    }

    #[test]
    fn test_zdle_rub0_rub1_encoding() {
        // Binary data with 0x7f, 0xff, and control characters
        let raw = vec![0x00, 0x18, 0x7f, 0xff, 0x11, 0x13, 0x8d, 0x42];
        let encoded = zdle_encode(&raw);

        // 0x7f must be encoded as ZDLE + ZRUB0 ('l')
        assert!(encoded.windows(2).any(|w| w == [ZDLE, ZRUB0]));
        // 0xff must be encoded as ZDLE + ZRUB1 ('m')
        assert!(encoded.windows(2).any(|w| w == [ZDLE, ZRUB1]));
        // 0x18 must be encoded as ZDLE + (0x18 ^ 0x40) ('X')
        assert!(encoded.windows(2).any(|w| w == [ZDLE, 0x58]));

        let (decoded, consumed) = zdle_decode(&encoded);
        assert_eq!(decoded, raw);
        assert_eq!(consumed, encoded.len());

        // Also test subpacket roundtrip with 32-bit CRC
        let subpacket = encode_subpacket(&raw, ZCRCG, true);
        let (sub_payload, frame_end, sub_consumed) =
            decode_subpacket(&subpacket, true).expect("decode binary subpacket");
        assert_eq!(sub_payload, raw);
        assert_eq!(frame_end, ZCRCG);
        assert_eq!(sub_consumed, subpacket.len());
    }

    #[test]
    fn test_zfile_header_clobber_flags() {
        let mut flags_in = [0u8; 4];
        flags_in[ZF0_IDX] = ZCBIN;
        flags_in[ZF1_IDX] = ZF1_ZMCLOB;
        let zfile_hdr = build_hex_header(ZFILE, flags_in);
        let (htype, flags, _start, end) = parse_hex_header(&zfile_hdr).expect("parse zfile header");
        assert_eq!(htype, ZFILE);
        assert_eq!(flags[ZF0_IDX], ZCBIN);
        assert_eq!(flags[ZF1_IDX], ZF1_ZMCLOB);
        assert_eq!(end, zfile_hdr.len());
    }

    #[test]
    fn test_zfile_header_prot_and_non_clobber_flags() {
        // Non-clobber (safe default)
        let mut flags_safe = [0u8; 4];
        flags_safe[ZF0_IDX] = ZCBIN;
        let zfile_hdr_safe = build_hex_header(ZFILE, flags_safe);
        let (_, flags_out, _, _) = parse_hex_header(&zfile_hdr_safe).expect("parse safe zfile");
        assert_eq!(flags_out[ZF0_IDX], ZCBIN);
        assert_eq!(flags_out[ZF1_IDX], 0);

        // Protected
        let mut flags_prot = [0u8; 4];
        flags_prot[ZF0_IDX] = ZCBIN;
        flags_prot[ZF1_IDX] = ZF1_ZMPROT;
        let zfile_hdr_prot = build_hex_header(ZFILE, flags_prot);
        let (_, flags_prot_out, _, _) = parse_hex_header(&zfile_hdr_prot).expect("parse prot zfile");
        assert_eq!(flags_prot_out[ZF0_IDX], ZCBIN);
        assert_eq!(flags_prot_out[ZF1_IDX], ZF1_ZMPROT);
    }

    #[test]
    fn test_zdle_encode_with_escctl() {
        // Test that 0x00 (NUL), 0x0D (CR), 0x0A (LF), 0x1B (ESC) are escaped under ESCCTL
        let raw = vec![0x00, 0x0D, 0x0A, 0x1B, 0x41, 0x7F, 0xFF, 0x80];
        let encoded_escctl = zdle_encode_with_escctl(&raw, true);

        // 0x00 must be escaped as ZDLE + (0x00 ^ 0x40) == [ZDLE, 0x40] ('@')
        assert!(encoded_escctl.windows(2).any(|w| w == [ZDLE, 0x40]));
        // 0x0D must be escaped as ZDLE + (0x0D ^ 0x40) == [ZDLE, 0x4D] ('M')
        assert!(encoded_escctl.windows(2).any(|w| w == [ZDLE, 0x4D]));
        // 0x41 ('A') must NOT be escaped
        assert!(encoded_escctl.contains(&0x41));

        // Decoding must recover exact raw bytes
        let (decoded, consumed) = zdle_decode(&encoded_escctl);
        assert_eq!(decoded, raw);
        assert_eq!(consumed, encoded_escctl.len());

        // Subpacket roundtrip with ESCCTL
        let subpacket = encode_subpacket_with_escctl(&raw, ZCRCW, false, true);
        let (sub_payload, frame_end, sub_consumed) =
            decode_subpacket(&subpacket, false).expect("decode binary subpacket with escctl");
        assert_eq!(sub_payload, raw);
        assert_eq!(frame_end, ZCRCW);
        assert_eq!(sub_consumed, subpacket.len());
    }
}
