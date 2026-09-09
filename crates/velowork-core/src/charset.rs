//! 全局通用终端字符集编码定义与辅助函数。
//!
//! 维护 Velowork 全局通用的标准字符集列表，并提供 WHATWG / IANA 别名智能规范化。

/// 全局支持的标准终端字符集编码列表（按常用频率与语系体系组织）。
pub const SUPPORTED_CHARSETS: &[&str] = &[
    // --- Unicode ---
    "UTF-8",
    "UTF-16LE",
    "UTF-16BE",
    "UTF-32LE",
    "UTF-32BE",
    // --- 中文 (Chinese) ---
    "GB18030",
    "GBK",
    "GB2312",
    "Big5",
    // --- 日韩 (Japanese / Korean) ---
    "Shift_JIS",
    "EUC-JP",
    "EUC-KR",
    // --- ISO-8859 系列 (ISO-8859-1 ~ 16) ---
    "ISO-8859-1",
    "ISO-8859-2",
    "ISO-8859-3",
    "ISO-8859-4",
    "ISO-8859-5",
    "ISO-8859-6",
    "ISO-8859-7",
    "ISO-8859-8",
    "ISO-8859-9",
    "ISO-8859-10",
    "ISO-8859-11",
    "ISO-8859-13",
    "ISO-8859-14",
    "ISO-8859-15",
    "ISO-8859-16",
    // --- Windows 系列 (Windows-1250 ~ 1258) ---
    "Windows-1250",
    "Windows-1251",
    "Windows-1252",
    "Windows-1253",
    "Windows-1254",
    "Windows-1255",
    "Windows-1256",
    "Windows-1257",
    "Windows-1258",
    // --- IBM / DOS / OEM 系列 ---
    "IBM850",
    "IBM860",
    "IBM874",
    // --- 斯拉夫 / 东南亚 / 历史与特定语系 ---
    "KOI8-R",
    "KOI8-U",
    "macintosh",
    "TIS-620",
    "TSCII",
    "hp-roman8",
    "WINSAMI2",
    "US-ASCII",
];

/// 默认字符集。
pub const DEFAULT_CHARSET: &str = "UTF-8";

/// 获取默认字符集。
pub fn default_charset() -> &'static str {
    DEFAULT_CHARSET
}

/// 检查给定字符集名称是否受支持（支持标准名称与常见别名）。
pub fn is_supported_charset(name: &str) -> bool {
    canonicalize_charset(name).is_some()
}

/// 尝试将任意字符集名称（含别名、不同大小写与连字符）规范化为 [`SUPPORTED_CHARSETS`] 中的标准大写标识符。
pub fn canonicalize_charset(name: &str) -> Option<&'static str> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 1. 精确不区分大小写匹配已支持列表
    for &supported in SUPPORTED_CHARSETS {
        if supported.eq_ignore_ascii_case(trimmed) {
            return Some(supported);
        }
    }

    // 2. 常见别名、无连字符变体及历史简称映射
    let normalized = trimmed.to_ascii_uppercase().replace(['-', '_', ' '], "");
    match normalized.as_str() {
        "UTF8" => return Some("UTF-8"),
        "UTF16LE" => return Some("UTF-16LE"),
        "UTF16BE" => return Some("UTF-16BE"),
        "UTF32LE" => return Some("UTF-32LE"),
        "UTF32BE" => return Some("UTF-32BE"),
        "UTF32" => return Some("UTF-32LE"),
        "GB18030" => return Some("GB18030"),
        "GBK" | "CP936" | "MS936" | "WINDOWS936" => return Some("GBK"),
        "GB2312" => return Some("GB2312"),
        "BIG5" | "CP950" | "WINDOWS950" => return Some("Big5"),
        "SJIS" | "SHIFTJIS" | "MS932" | "CP932" | "WINDOWS932" => return Some("Shift_JIS"),
        "EUCJP" => return Some("EUC-JP"),
        "EUCKR" | "CP949" => return Some("EUC-KR"),

        // ISO-8859
        "ISO88591" | "LATIN1" | "CSISOLATIN1" => return Some("ISO-8859-1"),
        "ISO88592" | "LATIN2" => return Some("ISO-8859-2"),
        "ISO88593" | "LATIN3" => return Some("ISO-8859-3"),
        "ISO88594" | "LATIN4" => return Some("ISO-8859-4"),
        "ISO88595" | "CYRILLIC" => return Some("ISO-8859-5"),
        "ISO88596" | "ARABIC" => return Some("ISO-8859-6"),
        "ISO88597" | "GREEK" => return Some("ISO-8859-7"),
        "ISO88598" | "HEBREW" => return Some("ISO-8859-8"),
        "ISO88599" | "LATIN5" => return Some("ISO-8859-9"),
        "ISO885910" | "LATIN6" => return Some("ISO-8859-10"),
        "ISO885911" => return Some("ISO-8859-11"),
        "ISO885913" | "LATIN7" => return Some("ISO-8859-13"),
        "ISO885914" | "LATIN8" => return Some("ISO-8859-14"),
        "ISO885915" | "LATIN9" => return Some("ISO-8859-15"),
        "ISO885916" | "LATIN10" => return Some("ISO-8859-16"),

        // Windows Code Pages
        "WINDOWS1250" | "CP1250" | "WIN1250" => return Some("Windows-1250"),
        "WINDOWS1251" | "CP1251" | "WIN1251" => return Some("Windows-1251"),
        "WINDOWS1252" | "CP1252" | "WIN1252" => return Some("Windows-1252"),
        "WINDOWS1253" | "CP1253" | "WIN1253" => return Some("Windows-1253"),
        "WINDOWS1254" | "CP1254" | "WIN1254" => return Some("Windows-1254"),
        "WINDOWS1255" | "CP1255" | "WIN1255" => return Some("Windows-1255"),
        "WINDOWS1256" | "CP1256" | "WIN1256" => return Some("Windows-1256"),
        "WINDOWS1257" | "CP1257" | "WIN1257" => return Some("Windows-1257"),
        "WINDOWS1258" | "CP1258" | "WIN1258" => return Some("Windows-1258"),

        // IBM / OEM
        "CP850" | "IBM850" | "850" => return Some("IBM850"),
        "CP860" | "IBM860" | "860" => return Some("IBM860"),
        "CP874" | "IBM874" | "WINDOWS874" | "WIN874" => return Some("IBM874"),

        // Others
        "KOI8R" => return Some("KOI8-R"),
        "KOI8U" => return Some("KOI8-U"),
        "MACROMAN" | "MAC" | "MACINTOSH" => return Some("macintosh"),
        "TIS620" => return Some("TIS-620"),
        "TSCII" => return Some("TSCII"),
        "HPROMAN8" | "ROMAN8" => return Some("hp-roman8"),
        "WINSAMI2" | "SAMI2" => return Some("WINSAMI2"),
        "USASCII" | "ASCII" => return Some("US-ASCII"),
        _ => {}
    }

    // 3. 借助 encoding_rs 进行 WHATWG 规范匹配
    if let Some(encoding) = encoding_rs::Encoding::for_label(trimmed.as_bytes()) {
        let name_lower = encoding.name().to_ascii_lowercase();
        for &supported in SUPPORTED_CHARSETS {
            if supported.to_ascii_lowercase() == name_lower {
                return Some(supported);
            }
        }
        if name_lower == "windows-1252" {
            return Some("Windows-1252");
        }
        if name_lower == "windows-874" {
            return Some("TIS-620");
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_charsets_has_utf8_default() {
        assert!(SUPPORTED_CHARSETS.contains(&"UTF-8"));
        assert_eq!(default_charset(), "UTF-8");
        assert!(is_supported_charset("UTF-8"));
        assert!(is_supported_charset("utf-8"));
        assert!(is_supported_charset("utf8"));
    }

    #[test]
    fn test_canonicalize_charset_comprehensive() {
        // Unicode
        assert_eq!(canonicalize_charset("utf-8"), Some("UTF-8"));
        assert_eq!(canonicalize_charset("UTF8"), Some("UTF-8"));
        assert_eq!(canonicalize_charset("utf-32le"), Some("UTF-32LE"));
        assert_eq!(canonicalize_charset("utf-32be"), Some("UTF-32BE"));

        // Chinese / Japanese / Korean
        assert_eq!(canonicalize_charset("gbk"), Some("GBK"));
        assert_eq!(canonicalize_charset("gb2312"), Some("GB2312"));
        assert_eq!(canonicalize_charset("gb_2312"), Some("GB2312"));
        assert_eq!(canonicalize_charset("gb-18030"), Some("GB18030"));
        assert_eq!(canonicalize_charset("big5"), Some("Big5"));
        assert_eq!(canonicalize_charset("sjis"), Some("Shift_JIS"));
        assert_eq!(canonicalize_charset("shift-jis"), Some("Shift_JIS"));
        assert_eq!(canonicalize_charset("euc-jp"), Some("EUC-JP"));
        assert_eq!(canonicalize_charset("euc-kr"), Some("EUC-KR"));

        // ISO-8859
        assert_eq!(canonicalize_charset("iso-8859-1"), Some("ISO-8859-1"));
        assert_eq!(canonicalize_charset("iso-8859-2"), Some("ISO-8859-2"));
        assert_eq!(canonicalize_charset("iso-8859-16"), Some("ISO-8859-16"));
        assert_eq!(canonicalize_charset("latin1"), Some("ISO-8859-1"));

        // Windows
        assert_eq!(canonicalize_charset("windows-1250"), Some("Windows-1250"));
        assert_eq!(canonicalize_charset("cp1251"), Some("Windows-1251"));
        assert_eq!(canonicalize_charset("windows-1258"), Some("Windows-1258"));

        // IBM & others
        assert_eq!(canonicalize_charset("cp850"), Some("IBM850"));
        assert_eq!(canonicalize_charset("ibm860"), Some("IBM860"));
        assert_eq!(canonicalize_charset("ibm874"), Some("IBM874"));
        assert_eq!(canonicalize_charset("tis-620"), Some("TIS-620"));
        assert_eq!(canonicalize_charset("koi8-u"), Some("KOI8-U"));
        assert_eq!(canonicalize_charset("macintosh"), Some("macintosh"));
        assert_eq!(canonicalize_charset("hp-roman8"), Some("hp-roman8"));
        assert_eq!(canonicalize_charset("winsami2"), Some("WINSAMI2"));
        assert_eq!(canonicalize_charset("tscii"), Some("TSCII"));

        assert_eq!(canonicalize_charset("unknown-xyz"), None);
    }
}
