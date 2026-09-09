//! Host 智能解析（Connection Parser Pipeline）。
//!
//! 支持从用户粘贴/输入的多种格式中解析出 host/port/username：
//! - OpenSSH 命令片段：`ssh -p 2222 root@host`
//! - URI：`ssh://user@host:port`、`scp://...`、`sftp://...`
//! - SCP：`user@host:/path`
//! - SFTP：`sftp user@host`
//! - Fallback：`user@host:port` / `host`
//!
//! 未来新增协议（如 `mosh://`）只需实现一个新的 [`ConnectionParser`] 并注册进
//! [`ConnectionParserPipeline`]，无需改动调用方或 UI。

/// 解析结果。字段为 `Option`，仅回填能确定的部分，避免覆盖用户已填内容。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedTarget {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
}

impl ParsedTarget {
    fn is_meaningful(&self) -> bool {
        self.host.is_some() || self.port.is_some() || self.username.is_some()
    }
}

/// 单个解析器契约。返回 `None` 表示当前解析器无法处理该输入。
pub trait ConnectionParser: Send + Sync {
    fn parse(&self, raw: &str) -> Option<ParsedTarget>;
}

/// 解析器串联管线：按注册顺序依次尝试，返回第一个有意义的结果。
pub struct ConnectionParserPipeline {
    parsers: Vec<Box<dyn ConnectionParser>>,
}

impl Default for ConnectionParserPipeline {
    fn default() -> Self {
        Self::with_builtins()
    }
}

impl ConnectionParserPipeline {
    /// 注册内建解析器（顺序：更具体的在前，Fallback 兜底在后）。
    pub fn with_builtins() -> Self {
        Self {
            parsers: vec![
                Box::new(OpenSshParser),
                Box::new(UriParser),
                Box::new(ScpParser),
                Box::new(SftpParser),
                Box::new(FallbackParser),
            ],
        }
    }

    pub fn register(&mut self, parser: Box<dyn ConnectionParser>) {
        // Fallback 应始终最后，故插入到末尾之前。
        let insert_at = self.parsers.len().saturating_sub(1);
        self.parsers.insert(insert_at, parser);
    }

    pub fn parse(&self, raw: &str) -> Option<ParsedTarget> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        for p in &self.parsers {
            if let Some(target) = p.parse(trimmed) {
                if target.is_meaningful() {
                    return Some(target);
                }
            }
        }
        None
    }
}

/// 从 `user@host` / `host` 分离出用户名与主机（不含端口）。
fn split_user_host(s: &str) -> (Option<String>, String) {
    if let Some((user, host)) = s.split_once('@') {
        let user = user.trim();
        let host = host.trim();
        if !user.is_empty() && !host.is_empty() {
            return (Some(user.to_string()), host.to_string());
        }
    }
    (None, s.trim().to_string())
}

/// 从 `host:port` 分离端口（仅当冒号后是合法端口数字时）。
fn split_host_port(s: &str) -> (String, Option<u16>) {
    // 处理 IPv6 [::1]:22 形式
    if let Some(rest) = s.strip_prefix('[') {
        if let Some((addr, tail)) = rest.split_once(']') {
            let port = tail.strip_prefix(':').and_then(|p| p.trim().parse::<u16>().ok());
            return (addr.trim().to_string(), port);
        }
    }
    if let Some((host, port)) = s.rsplit_once(':') {
        if let Ok(p) = port.trim().parse::<u16>() {
            return (host.trim().to_string(), Some(p));
        }
    }
    (s.trim().to_string(), None)
}

/// OpenSSH 命令片段解析：`ssh [-p PORT] [-l USER] [user@]host`。
pub struct OpenSshParser;

impl ConnectionParser for OpenSshParser {
    fn parse(&self, raw: &str) -> Option<ParsedTarget> {
        let tokens: Vec<&str> = raw.split_whitespace().collect();
        if tokens.is_empty() || !tokens[0].eq_ignore_ascii_case("ssh") {
            return None;
        }

        let mut result = ParsedTarget::default();
        let mut i = 1;
        while i < tokens.len() {
            let tok = tokens[i];
            match tok {
                "-p" => {
                    if let Some(v) = tokens.get(i + 1) {
                        result.port = v.trim().parse::<u16>().ok();
                        i += 2;
                        continue;
                    }
                }
                "-l" => {
                    if let Some(v) = tokens.get(i + 1) {
                        result.username = Some(v.trim().to_string());
                        i += 2;
                        continue;
                    }
                }
                _ if tok.starts_with("-p") && tok.len() > 2 => {
                    result.port = tok[2..].trim().parse::<u16>().ok();
                }
                _ if tok.starts_with('-') => {
                    // 跳过其它选项（可能带参数，简单跳过自身）。
                }
                _ => {
                    // 目标 host（可能含 user@ 与 :port）
                    let (user, hostport) = split_user_host(tok);
                    let (host, port) = split_host_port(&hostport);
                    if user.is_some() {
                        result.username = user;
                    }
                    if !host.is_empty() {
                        result.host = Some(host);
                    }
                    if port.is_some() {
                        result.port = port;
                    }
                }
            }
            i += 1;
        }
        Some(result)
    }
}

/// URI 解析：`ssh://`、`scp://`、`sftp://`。
pub struct UriParser;

impl ConnectionParser for UriParser {
    fn parse(&self, raw: &str) -> Option<ParsedTarget> {
        let schemes = ["ssh://", "scp://", "sftp://"];
        let rest = schemes
            .iter()
            .find_map(|s| raw.strip_prefix(*s))?;
        // 去掉 path 部分
        let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
        let (user, hostport) = split_user_host(authority);
        let (host, port) = split_host_port(&hostport);
        Some(ParsedTarget {
            host: if host.is_empty() { None } else { Some(host) },
            port,
            username: user,
        })
    }
}

/// SCP 形式：`user@host:/path` 或 `user@host:path`（冒号后非端口）。
pub struct ScpParser;

impl ConnectionParser for ScpParser {
    fn parse(&self, raw: &str) -> Option<ParsedTarget> {
        // 必须包含 '@' 和 ':'，且冒号后不是纯端口（否则交给 Fallback）
        if !raw.contains('@') || !raw.contains(':') || raw.contains(' ') {
            return None;
        }
        let (user, hostpart) = split_user_host(raw);
        user.as_ref()?;
        // 取冒号前作为 host（scp 冒号后是路径）
        let (host, _path) = hostpart.split_once(':')?;
        let host = host.trim();
        if host.is_empty() {
            return None;
        }
        // 若冒号后是纯数字端口则不当作 scp（让 fallback 处理端口）
        if let Some((_, tail)) = hostpart.split_once(':') {
            if tail.trim().parse::<u16>().is_ok() {
                return None;
            }
        }
        Some(ParsedTarget {
            host: Some(host.to_string()),
            port: None,
            username: user,
        })
    }
}

/// `sftp user@host` 命令形式。
pub struct SftpParser;

impl ConnectionParser for SftpParser {
    fn parse(&self, raw: &str) -> Option<ParsedTarget> {
        let tokens: Vec<&str> = raw.split_whitespace().collect();
        if tokens.len() < 2 || !tokens[0].eq_ignore_ascii_case("sftp") {
            return None;
        }
        let target = tokens.last()?;
        let (user, hostport) = split_user_host(target);
        let (host, port) = split_host_port(&hostport);
        Some(ParsedTarget {
            host: if host.is_empty() { None } else { Some(host) },
            port,
            username: user,
        })
    }
}

/// 兜底解析：`user@host:port` / `host:port` / `host`。
pub struct FallbackParser;

impl ConnectionParser for FallbackParser {
    fn parse(&self, raw: &str) -> Option<ParsedTarget> {
        if raw.contains(' ') {
            return None;
        }
        let (user, hostport) = split_user_host(raw);
        let (host, port) = split_host_port(&hostport);
        Some(ParsedTarget {
            host: if host.is_empty() { None } else { Some(host) },
            port,
            username: user,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_at_host() {
        let p = ConnectionParserPipeline::with_builtins();
        let r = p.parse("root@example.com").unwrap();
        assert_eq!(r.username.as_deref(), Some("root"));
        assert_eq!(r.host.as_deref(), Some("example.com"));
    }

    #[test]
    fn parses_user_host_port() {
        let p = ConnectionParserPipeline::with_builtins();
        let r = p.parse("root@example.com:2222").unwrap();
        assert_eq!(r.username.as_deref(), Some("root"));
        assert_eq!(r.host.as_deref(), Some("example.com"));
        assert_eq!(r.port, Some(2222));
    }

    #[test]
    fn parses_openssh_command() {
        let p = ConnectionParserPipeline::with_builtins();
        let r = p.parse("ssh -p 2222 root@example.com").unwrap();
        assert_eq!(r.username.as_deref(), Some("root"));
        assert_eq!(r.host.as_deref(), Some("example.com"));
        assert_eq!(r.port, Some(2222));
    }

    #[test]
    fn parses_ssh_uri() {
        let p = ConnectionParserPipeline::with_builtins();
        let r = p.parse("ssh://admin@10.0.0.1:22/path").unwrap();
        assert_eq!(r.username.as_deref(), Some("admin"));
        assert_eq!(r.host.as_deref(), Some("10.0.0.1"));
        assert_eq!(r.port, Some(22));
    }

    #[test]
    fn parses_bare_host() {
        let p = ConnectionParserPipeline::with_builtins();
        let r = p.parse("example.com").unwrap();
        assert_eq!(r.host.as_deref(), Some("example.com"));
        assert_eq!(r.username, None);
    }
}
