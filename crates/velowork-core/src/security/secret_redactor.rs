//! Structured secret redaction and sanitization for terminal commands, history, and AI contexts.

/// Placeholder string used to mask detected sensitive values.
pub const REDACTED_PLACEHOLDER: &str = "<redacted>";

/// Engine for detecting and redacting sensitive data in commands.
pub struct SecretRedactor;

impl SecretRedactor {
    /// Sanitizes a command string by redacting known sensitive patterns (passwords, tokens, API keys, basic auth credentials).
    pub fn redact_command(command: &str) -> String {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return String::new();
        }

        let mut sanitized = trimmed.to_string();

        // 1. URL Credentials: e.g. https://user:password@example.com or postgres://user:password@localhost:5432/db
        sanitized = redact_url_credentials(&sanitized);

        // 2. HTTP Authorization Header: e.g. Authorization: Bearer xxx or Authorization: Basic xxx
        sanitized = redact_authorization_headers(&sanitized);

        // 3. Curl user auth: curl -u user:password or --user user:password
        sanitized = redact_curl_user_auth(&sanitized);

        // 4. Piped secrets: e.g. echo "secret" | sudo ... or echo secret | sudo ...
        sanitized = redact_piped_secrets(&sanitized);

        // 5. CLI Flags & Key-Value Arguments: e.g. --password=xxx, --password xxx, -pSecret, PASSWORD=xxx, etc.
        sanitized = redact_flags_and_env_secrets(&sanitized);

        sanitized
    }

    /// Determines if a command should be recorded to history based on ignored commands list and space-prefix rules.
    pub fn should_record_to_history(
        raw_command: &str,
        ignored_commands: &[String],
        ignore_space: bool,
    ) -> bool {
        if raw_command.is_empty() {
            return false;
        }

        // 1. If ignore_space is enabled, skip commands starting with a leading space
        if ignore_space && raw_command.starts_with(' ') {
            return false;
        }

        let trimmed = raw_command.trim();
        if trimmed.is_empty() {
            return false;
        }

        // 2. Check if the command is a single bare word with no arguments
        let is_single_word = !trimmed.contains(char::is_whitespace);
        if is_single_word {
            let trimmed_lower = trimmed.to_lowercase();
            if ignored_commands.iter().any(|cmd| cmd.trim().eq_ignore_ascii_case(&trimmed_lower)) {
                return false;
            }
        }

        true
    }
}

/// Redact credentials embedded in URLs: `scheme://user:password@host` -> `scheme://user:<redacted>@host`
fn redact_url_credentials(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut remainder = input;

    while let Some(proto_idx) = remainder.find("://") {
        result.push_str(&remainder[..proto_idx + 3]);
        remainder = &remainder[proto_idx + 3..];

        // Find next boundary (@ or whitespace or quote or end)
        let end_bound = remainder.find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '`' || c == ';')
            .unwrap_or(remainder.len());
        let url_part = &remainder[..end_bound];

        if let Some(at_idx) = url_part.find('@') {
            let userinfo = &url_part[..at_idx];
            let host_part = &url_part[at_idx..];
            if let Some(colon_idx) = userinfo.find(':') {
                let username = &userinfo[..colon_idx];
                result.push_str(username);
                result.push(':');
                result.push_str(REDACTED_PLACEHOLDER);
                result.push_str(host_part);
            } else {
                result.push_str(url_part);
            }
        } else {
            result.push_str(url_part);
        }

        remainder = &remainder[end_bound..];
    }

    result.push_str(remainder);
    result
}

/// Redact Authorization headers: `Authorization: Bearer <token>` or `Authorization: Basic <token>`
fn redact_authorization_headers(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut remainder = input;

    while let Some(auth_idx) = find_case_insensitive(remainder, "authorization:") {
        let auth_end = auth_idx + "authorization:".len();
        result.push_str(&remainder[..auth_end]);
        remainder = &remainder[auth_end..];

        // Check for Bearer or Basic
        let trimmed_lead = remainder.trim_start();
        let leading_spaces = remainder.len() - trimmed_lead.len();
        result.push_str(&remainder[..leading_spaces]);
        remainder = trimmed_lead;

        if let Some(bearer_idx) = find_case_insensitive(remainder, "bearer ")
            && bearer_idx == 0 {
            result.push_str("Bearer ");
            remainder = &remainder["bearer ".len()..];
            remainder = consume_and_redact_token(remainder, &mut result);
            continue;
        }

        if let Some(basic_idx) = find_case_insensitive(remainder, "basic ")
            && basic_idx == 0 {
            result.push_str("Basic ");
            remainder = &remainder["basic ".len()..];
            remainder = consume_and_redact_token(remainder, &mut result);
            continue;
        }
    }

    result.push_str(remainder);
    result
}

/// Redact curl `-u user:pass` or `--user user:pass`
fn redact_curl_user_auth(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut remainder = input;

    while let Some(idx) = find_word_flag(remainder, "-u").or_else(|| find_word_flag(remainder, "--user")) {
        let flag_len = if remainder[idx..].starts_with("--user") { 6 } else { 2 };
        result.push_str(&remainder[..idx + flag_len]);
        remainder = &remainder[idx + flag_len..];

        let trimmed_lead = remainder.trim_start();
        let leading_spaces = remainder.len() - trimmed_lead.len();
        result.push_str(&remainder[..leading_spaces]);
        remainder = trimmed_lead;

        // Extract the user:pass arg
        let (quote_char, raw_val, after_val) = extract_argument_token(remainder);
        if let Some(colon_idx) = raw_val.find(':') {
            let user = &raw_val[..colon_idx];
            if let Some(q) = quote_char {
                result.push(q);
            }
            result.push_str(user);
            result.push(':');
            result.push_str(REDACTED_PLACEHOLDER);
            if let Some(q) = quote_char {
                result.push(q);
            }
        } else {
            // No colon, output as-is
            if let Some(q) = quote_char {
                result.push(q);
            }
            result.push_str(&raw_val);
            if let Some(q) = quote_char {
                result.push(q);
            }
        }
        remainder = after_val;
    }

    result.push_str(remainder);
    result
}

/// Redact `echo "password" | sudo` or `echo 'password' | sudo` or `echo password | sudo -S`
fn redact_piped_secrets(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut remainder = input;

    while let Some(pipe_sudo_idx) = find_case_insensitive(remainder, "| sudo")
        .or_else(|| find_case_insensitive(remainder, "|sudo"))
    {
        let before_pipe = &remainder[..pipe_sudo_idx];
        let after_pipe = &remainder[pipe_sudo_idx..];

        if let Some(echo_idx) = find_case_insensitive(before_pipe, "echo ") {
            let echo_end = echo_idx + 5;
            result.push_str(&before_pipe[..echo_end]);
            let secret_part = before_pipe[echo_end..].trim();
            if !secret_part.is_empty() {
                if secret_part.starts_with('"') && secret_part.ends_with('"') {
                    result.push_str(&format!("\"{}\" ", REDACTED_PLACEHOLDER));
                } else if secret_part.starts_with('\'') && secret_part.ends_with('\'') {
                    result.push_str(&format!("'{}' ", REDACTED_PLACEHOLDER));
                } else {
                    result.push_str(&format!("{} ", REDACTED_PLACEHOLDER));
                }
            }
            result.push_str(after_pipe);
            return result;
        } else {
            result.push_str(&remainder[..pipe_sudo_idx + 1]);
            remainder = &remainder[pipe_sudo_idx + 1..];
        }
    }

    result.push_str(remainder);
    result
}

/// Sensitive flag names (checked case-insensitively)
const SENSITIVE_FLAG_NAMES: &[&str] = &[
    "--password",
    "--passwd",
    "--pass",
    "--token",
    "--api-key",
    "--apikey",
    "--secret",
    "--secret-key",
    "--secretkey",
    "--access-token",
    "--auth-token",
    "--private-key",
];

/// Short flags that take a secret value (e.g. `-p<secret>` for mysql/sshpass)
const SENSITIVE_SHORT_FLAGS: &[&str] = &[
    "-p",
    "-t",
];

/// Redact CLI flags (`--password=xxx`, `--password xxx`, `-pSecret`) and environment variable assignments (`PASSWORD=xxx`).
fn redact_flags_and_env_secrets(input: &str) -> String {
    let mut tokens = tokenize_command(input);
    let mut i = 0;

    while i < tokens.len() {
        let token = tokens[i].text.clone();
        let token_lower = token.to_lowercase();

        // 1. Check for sensitive flag with '=' (e.g. `--password=secret` or `PASSWORD=secret`)
        if let Some(eq_idx) = token.find('=') {
            let key = &token[..eq_idx];
            let key_lower = key.to_lowercase();
            let is_sensitive_key = SENSITIVE_FLAG_NAMES.iter().any(|f| f.eq_ignore_ascii_case(&key_lower))
                || key_lower == "password"
                || key_lower == "passwd"
                || key_lower == "secret"
                || key_lower == "token"
                || key_lower == "api_key"
                || key_lower == "apikey"
                || key_lower == "access_key"
                || key_lower == "auth_token";

            if is_sensitive_key {
                tokens[i].text = format!("{}={}", key, REDACTED_PLACEHOLDER);
                i += 1;
                continue;
            }
        }

        // 2. Check for sensitive long flag followed by separate value (e.g. `--password` `secret`)
        if SENSITIVE_FLAG_NAMES.iter().any(|f| f.eq_ignore_ascii_case(&token_lower))
            && i + 1 < tokens.len() && !tokens[i + 1].text.starts_with('-') {
            tokens[i + 1].text = REDACTED_PLACEHOLDER.to_string();
            i += 2;
            continue;
        }

        // 3. Check for sensitive short flags:
        // Case A: Attached value, e.g. `-pMySecret`
        let mut short_flag_handled = false;
        for &sf in SENSITIVE_SHORT_FLAGS {
            if token.starts_with(sf) && token.len() > sf.len() && !token.starts_with("--") {
                tokens[i].text = format!("{}{}", sf, REDACTED_PLACEHOLDER);
                short_flag_handled = true;
                break;
            }
        }
        if short_flag_handled {
            i += 1;
            continue;
        }

        // Case B: Separate value, e.g. `-p` `MySecret`
        if SENSITIVE_SHORT_FLAGS.contains(&token.as_str())
            && i + 1 < tokens.len() && !tokens[i + 1].text.starts_with('-') {
            tokens[i + 1].text = REDACTED_PLACEHOLDER.to_string();
            i += 2;
            continue;
        }

        i += 1;
    }

    reconstruct_tokens(&tokens)
}

#[derive(Clone, Debug)]
struct CommandToken {
    text: String,
    leading_whitespace: String,
    quote: Option<char>,
}

fn tokenize_command(cmd: &str) -> Vec<CommandToken> {
    let mut tokens = Vec::new();
    let mut chars = cmd.chars().peekable();

    while chars.peek().is_some() {
        // Collect leading whitespace
        let mut whitespace = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                whitespace.push(c);
                chars.next();
            } else {
                break;
            }
        }

        if chars.peek().is_none() {
            if !whitespace.is_empty() {
                tokens.push(CommandToken {
                    text: String::new(),
                    leading_whitespace: whitespace,
                    quote: None,
                });
            }
            break;
        }

        // Parse token
        let mut token_text = String::new();
        let mut quote = None;

        if let Some(&c) = chars.peek() {
            if c == '"' || c == '\'' {
                quote = Some(c);
                chars.next();
                while let Some(&tc) = chars.peek() {
                    chars.next();
                    if tc == c {
                        break;
                    }
                    token_text.push(tc);
                }
            } else {
                while let Some(&tc) = chars.peek() {
                    if tc.is_whitespace() {
                        break;
                    }
                    token_text.push(tc);
                    chars.next();
                }
            }
        }

        tokens.push(CommandToken {
            text: token_text,
            leading_whitespace: whitespace,
            quote,
        });
    }

    tokens
}

fn reconstruct_tokens(tokens: &[CommandToken]) -> String {
    let mut result = String::new();
    for token in tokens {
        result.push_str(&token.leading_whitespace);
        if let Some(q) = token.quote {
            result.push(q);
            result.push_str(&token.text);
            result.push(q);
        } else {
            result.push_str(&token.text);
        }
    }
    result
}

fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let lower_h = haystack.to_lowercase();
    let lower_n = needle.to_lowercase();
    lower_h.find(&lower_n)
}

fn find_word_flag(haystack: &str, flag: &str) -> Option<usize> {
    let mut search_start = 0;
    while let Some(idx) = haystack[search_start..].find(flag) {
        let actual_idx = search_start + idx;
        let is_start = actual_idx == 0 || haystack[..actual_idx].chars().next_back().is_none_or(|c| c.is_whitespace() || c == ';');
        let after_idx = actual_idx + flag.len();
        let is_end = after_idx == haystack.len() || haystack[after_idx..].chars().next().is_none_or(|c| c.is_whitespace() || c == '=');

        if is_start && is_end {
            return Some(actual_idx);
        }
        search_start = actual_idx + flag.len();
    }
    None
}

fn extract_argument_token(input: &str) -> (Option<char>, String, &str) {
    if input.is_empty() {
        return (None, String::new(), input);
    }

    let first_char = input.chars().next().unwrap_or(' ');
    if first_char == '"' || first_char == '\'' {
        let quote = first_char;
        let content_start = 1;
        if let Some(close_idx) = input[content_start..].find(quote) {
            let val = &input[content_start..content_start + close_idx];
            let after = &input[content_start + close_idx + 1..];
            (Some(quote), val.to_string(), after)
        } else {
            (Some(quote), input[content_start..].to_string(), "")
        }
    } else {
        let end_idx = input.find(|c: char| c.is_whitespace() || c == ';' || c == '|' || c == '"' || c == '\'').unwrap_or(input.len());
        (None, input[..end_idx].to_string(), &input[end_idx..])
    }
}

fn consume_and_redact_token<'a>(remainder: &'a str, result: &mut String) -> &'a str {
    let (quote_char, _val, after) = extract_argument_token(remainder);
    if let Some(q) = quote_char {
        result.push(q);
    }
    result.push_str(REDACTED_PLACEHOLDER);
    if let Some(q) = quote_char {
        result.push(q);
    }
    after
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_url_credentials() {
        assert_eq!(
            SecretRedactor::redact_command("curl https://user:mysecretpassword@api.github.com/v1"),
            "curl https://user:<redacted>@api.github.com/v1"
        );
        assert_eq!(
            SecretRedactor::redact_command("psql postgres://postgres:supersecret@127.0.0.1:5432/mydb"),
            "psql postgres://postgres:<redacted>@127.0.0.1:5432/mydb"
        );
        assert_eq!(
            SecretRedactor::redact_command("git clone https://user:token@gitlab.com/org/repo.git"),
            "git clone https://user:<redacted>@gitlab.com/org/repo.git"
        );
    }

    #[test]
    fn test_redact_authorization_headers() {
        assert_eq!(
            SecretRedactor::redact_command("curl -H \"Authorization: Bearer eyJhbGciOi...\" https://api.com"),
            "curl -H \"Authorization: Bearer <redacted>\" https://api.com"
        );
        assert_eq!(
            SecretRedactor::redact_command("curl -H 'Authorization: Basic dXNlcjpwYXNz' https://api.com"),
            "curl -H 'Authorization: Basic <redacted>' https://api.com"
        );
    }

    #[test]
    fn test_redact_curl_user_auth() {
        assert_eq!(
            SecretRedactor::redact_command("curl -u admin:123456 https://example.com"),
            "curl -u admin:<redacted> https://example.com"
        );
        assert_eq!(
            SecretRedactor::redact_command("curl --user 'admin:superpass' https://example.com"),
            "curl --user 'admin:<redacted>' https://example.com"
        );
    }

    #[test]
    fn test_redact_piped_secrets() {
        assert_eq!(
            SecretRedactor::redact_command("echo \"my_password\" | sudo -S systemctl restart nginx"),
            "echo \"<redacted>\" | sudo -S systemctl restart nginx"
        );
        assert_eq!(
            SecretRedactor::redact_command("echo 'supersecret' | sudo apt update"),
            "echo '<redacted>' | sudo apt update"
        );
    }

    #[test]
    fn test_redact_cli_flags_and_env() {
        assert_eq!(
            SecretRedactor::redact_command("mysql -u root -p123456 -h 127.0.0.1"),
            "mysql -u root -p<redacted> -h 127.0.0.1"
        );
        assert_eq!(
            SecretRedactor::redact_command("mysql -u root -p 123456"),
            "mysql -u root -p <redacted>"
        );
        assert_eq!(
            SecretRedactor::redact_command("mysqldump --password=secret_db_pass mydb > dump.sql"),
            "mysqldump --password=<redacted> mydb > dump.sql"
        );
        assert_eq!(
            SecretRedactor::redact_command("mysqldump --password secret_db_pass mydb"),
            "mysqldump --password <redacted> mydb"
        );
        assert_eq!(
            SecretRedactor::redact_command("sshpass -p 'my_ssh_pass' ssh root@192.168.1.1"),
            "sshpass -p '<redacted>' ssh root@192.168.1.1"
        );
        assert_eq!(
            SecretRedactor::redact_command("gh auth login --token ghp_1234567890abcdef"),
            "gh auth login --token <redacted>"
        );
        assert_eq!(
            SecretRedactor::redact_command("PASSWORD=supersecret ./start.sh"),
            "PASSWORD=<redacted> ./start.sh"
        );
        assert_eq!(
            SecretRedactor::redact_command("TOKEN=my_api_token node server.js"),
            "TOKEN=<redacted> node server.js"
        );
    }

    #[test]
    fn test_normal_commands_untouched() {
        assert_eq!(
            SecretRedactor::redact_command("ls -la /var/log"),
            "ls -la /var/log"
        );
        assert_eq!(
            SecretRedactor::redact_command("git status"),
            "git status"
        );
        assert_eq!(
            SecretRedactor::redact_command("cargo check --workspace"),
            "cargo check --workspace"
        );
    }

    #[test]
    fn test_should_record_to_history() {
        let ignored: Vec<String> = vec![
            "ls".to_string(),
            "ll".to_string(),
            "la".to_string(),
            "l".to_string(),
            "pwd".to_string(),
            "clear".to_string(),
            "cls".to_string(),
            "exit".to_string(),
            "history".to_string(),
        ];

        // 1. Bare trivial commands should be excluded
        assert!(!SecretRedactor::should_record_to_history("ls", &ignored, true));
        assert!(!SecretRedactor::should_record_to_history("ll", &ignored, true));
        assert!(!SecretRedactor::should_record_to_history("pwd", &ignored, true));
        assert!(!SecretRedactor::should_record_to_history("clear", &ignored, true));
        assert!(!SecretRedactor::should_record_to_history("cls", &ignored, true));
        assert!(!SecretRedactor::should_record_to_history("exit", &ignored, true));
        assert!(!SecretRedactor::should_record_to_history("history", &ignored, true));
        assert!(!SecretRedactor::should_record_to_history("  ls  ", &ignored, false));

        // 2. Commands with arguments should NOT be excluded
        assert!(SecretRedactor::should_record_to_history("ls -la /var/log", &ignored, true));
        assert!(SecretRedactor::should_record_to_history("pwd -P", &ignored, true));
        assert!(SecretRedactor::should_record_to_history("clear -x", &ignored, true));
        assert!(SecretRedactor::should_record_to_history("docker ps", &ignored, true));
        assert!(SecretRedactor::should_record_to_history("git status", &ignored, true));

        // 3. Space-prefixed commands
        assert!(!SecretRedactor::should_record_to_history(" ls -la", &ignored, true));
        assert!(!SecretRedactor::should_record_to_history(" secret_command", &ignored, true));
        // If ignore_space is false, leading space is recorded (as long as not empty)
        assert!(SecretRedactor::should_record_to_history(" ls -la", &ignored, false));

        // 4. Empty commands
        assert!(!SecretRedactor::should_record_to_history("", &ignored, true));
        assert!(!SecretRedactor::should_record_to_history("   ", &ignored, true));
    }
}
