//! TOTP (Time-Based One-Time Password, RFC 6238 / RFC 4226) and Prompt Recognition
//! for SSH Multi-Round Keyboard-Interactive Authentication.

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, PartialEq, Eq)]
pub enum TotpError {
    InvalidBase32,
    EmptySecret,
}

impl std::fmt::Display for TotpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TotpError::InvalidBase32 => write!(f, "Invalid Base32 secret key"),
            TotpError::EmptySecret => write!(f, "TOTP secret key is empty"),
        }
    }
}

impl std::error::Error for TotpError {}

/// Generates a standard 6-digit TOTP code from a Base32 secret string for the current time.
pub fn generate_current_totp(secret_base32: &str) -> Result<String, TotpError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    generate_totp(secret_base32, now)
}

/// Generates a standard 6-digit TOTP code (RFC 6238) for a given Unix timestamp in seconds.
pub fn generate_totp(secret_base32: &str, timestamp_secs: u64) -> Result<String, TotpError> {
    let key = decode_base32(secret_base32)?;
    if key.is_empty() {
        return Err(TotpError::EmptySecret);
    }

    let time_step = 30u64;
    let counter = timestamp_secs / time_step;
    let counter_bytes = counter.to_be_bytes();

    let hash = hmac_sha1(&key, &counter_bytes);

    // Dynamic truncation (RFC 4226 Section 5.4)
    let offset = (hash[19] & 0x0f) as usize;
    let binary = ((hash[offset] as u32 & 0x7f) << 24)
        | ((hash[offset + 1] as u32) << 16)
        | ((hash[offset + 2] as u32) << 8)
        | (hash[offset + 3] as u32);

    let code = binary % 1_000_000;
    Ok(format!("{:06}", code))
}

/// Decode standard Base32 string (RFC 4648) into bytes.
///
/// Ignores whitespace, hyphens, and padding characters (`=`).
pub fn decode_base32(input: &str) -> Result<Vec<u8>, TotpError> {
    let mut clean = String::with_capacity(input.len());
    for c in input.chars() {
        if c.is_ascii_whitespace() || c == '-' || c == '=' {
            continue;
        }
        clean.push(c.to_ascii_uppercase());
    }

    if clean.is_empty() {
        return Err(TotpError::EmptySecret);
    }

    let mut out = Vec::with_capacity(clean.len() * 5 / 8);
    let mut buffer: u32 = 0;
    let mut bits_in_buffer: u8 = 0;

    for ch in clean.chars() {
        let val = match ch {
            'A'..='Z' => (ch as u8 - b'A') as u32,
            '2'..='7' => (ch as u8 - b'2' + 26) as u32,
            _ => return Err(TotpError::InvalidBase32),
        };

        buffer = (buffer << 5) | val;
        bits_in_buffer += 5;

        if bits_in_buffer >= 8 {
            bits_in_buffer -= 8;
            out.push((buffer >> bits_in_buffer) as u8);
            buffer &= (1 << bits_in_buffer) - 1;
        }
    }

    Ok(out)
}

/// Computes HMAC-SHA1(key, message).
fn hmac_sha1(key: &[u8], message: &[u8]) -> [u8; 20] {
    let block_size = 64;
    let mut key_buf = [0u8; 64];

    if key.len() > block_size {
        let key_hash = sha1(key);
        key_buf[..20].copy_from_slice(&key_hash);
    } else {
        key_buf[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];

    for i in 0..64 {
        ipad[i] ^= key_buf[i];
        opad[i] ^= key_buf[i];
    }

    // Inner hash = SHA1(ipad || message)
    let mut inner_input = Vec::with_capacity(64 + message.len());
    inner_input.extend_from_slice(&ipad);
    inner_input.extend_from_slice(message);
    let inner_hash = sha1(&inner_input);

    // Outer hash = SHA1(opad || inner_hash)
    let mut outer_input = Vec::with_capacity(64 + 20);
    outer_input.extend_from_slice(&opad);
    outer_input.extend_from_slice(&inner_hash);
    sha1(&outer_input)
}

/// Computes SHA-1 hash of data.
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h0 = 0x67452301u32;
    let mut h1 = 0xEFCDAB89u32;
    let mut h2 = 0x98BADCFEu32;
    let mut h3 = 0x10325476u32;
    let mut h4 = 0xC3D2E1F0u32;

    let len_bits = (data.len() as u64).wrapping_mul(8);

    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&len_bits.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for (i, item) in w.iter().enumerate().take(80) {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*item);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

/// Checks whether a server prompt is requesting an OTP / 2FA / Verification code.
pub fn is_otp_prompt(prompt: &str) -> bool {
    let p = prompt.trim().to_lowercase();
    p.contains("otp")
        || p.contains("totp")
        || p.contains("token")
        || p.contains("verification code")
        || p.contains("verification")
        || p.contains("authenticator")
        || p.contains("2fa")
        || p.contains("mfa")
        || p.contains("duo")
        || p.contains("one-time")
        || p.contains("security code")
        || p.contains("动态码")
        || p.contains("动态口令")
        || p.contains("验证码")
        || p.contains("一次性口令")
        || p.contains("两步验证")
        || p.contains("双因素")
}

/// Checks whether a server prompt is requesting a standard user password.
pub fn is_password_prompt(prompt: &str) -> bool {
    let p = prompt.trim().to_lowercase();
    // Must contain password / passcode / 密码 and NOT be an OTP/token/verification prompt
    (p.contains("password") || p.contains("passcode") || p.contains("密码"))
        && !is_otp_prompt(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base32_decoding() {
        // "Hello!" in Base32 is "JBSWY3DPEE======"
        let decoded = decode_base32("JBSWY3DPEE").expect("valid base32");
        assert_eq!(String::from_utf8(decoded).unwrap(), "Hello!");

        // With spaces, hyphens, lowercase
        let decoded2 = decode_base32("jbsw-y3dp-ee").expect("valid base32");
        assert_eq!(String::from_utf8(decoded2).unwrap(), "Hello!");
    }

    #[test]
    fn test_rfc6238_totp_vectors() {
        // RFC 6238 Test Vector for SHA1 with secret "12345678901234567890" (in ASCII)
        // "12345678901234567890" Base32 = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"
        let secret = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

        // T0 = 59s -> Time step counter = 1 -> Expected code = "287082"
        let code1 = generate_totp(secret, 59).expect("generate totp");
        assert_eq!(code1, "287082");

        // T = 1111111109s -> Expected code = "081804"
        let code2 = generate_totp(secret, 1111111109).expect("generate totp");
        assert_eq!(code2, "081804");

        // T = 1111111111s -> Expected code = "050471"
        let code3 = generate_totp(secret, 1111111111).expect("generate totp");
        assert_eq!(code3, "050471");

        // T = 1234567890s -> Expected code = "005924"
        let code4 = generate_totp(secret, 1234567890).expect("generate totp");
        assert_eq!(code4, "005924");
    }

    #[test]
    fn test_prompt_recognition() {
        assert!(is_otp_prompt("Enter Verification Code: "));
        assert!(is_otp_prompt("OTP: "));
        assert!(is_otp_prompt("Duo passcode or ('push' to push): "));
        assert!(is_otp_prompt("请输入动态口令: "));
        assert!(is_otp_prompt("Google Authenticator Code: "));
        assert!(is_otp_prompt("2FA Token: "));

        assert!(is_password_prompt("Password: "));
        assert!(is_password_prompt("user@192.168.1.1's password: "));
        assert!(is_password_prompt("请输入密码："));

        assert!(!is_password_prompt("Enter OTP / Password token: "));
    }
}
