//! zstd 压缩封装（Bundle 使用，非密码学原语）。

use crate::error::{SecurityError, Result};

/// 压缩数据（zstd level 3）。
pub fn compress(data: &[u8]) -> Result<Vec<u8>> {
    zstd::encode_all(data, 3).map_err(|e| SecurityError::crypto(format!("zstd compress: {e}")))
}

/// 解压数据。
pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    zstd::decode_all(data).map_err(|e| SecurityError::crypto(format!("zstd decompress: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zstd_round_trip() {
        let data = b"velowork-bundle-payload-repeated".repeat(8);
        let c = compress(&data).unwrap();
        let d = decompress(&c).unwrap();
        assert_eq!(d, data);
    }
}
