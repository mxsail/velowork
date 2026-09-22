//! Shell integration for OSC 133 prompt markers and command lifecycle tracking.
//!
//! Provides scripts and utilities for emitting OSC 133 sequences across different shells:
//! - `OSC 133 ; A \a` -> PromptStart
//! - `OSC 133 ; B \a` -> CommandStart
//! - `OSC 133 ; C \a` -> CommandExecuted
//! - `OSC 133 ; D [; exit_code] \a` -> CommandFinished
//!
//! Also provides a lightweight sentinel-marker wrapper mechanism for shells or remote
//! SSH sessions where OSC 133 shell integration is not available or enabled.

/// Bash shell integration snippet.
pub const BASH_INTEGRATION_SCRIPT: &str = r#"
__velowork_prompt_start() {
    printf "\033]133;A\007"
}
__velowork_prompt_end() {
    printf "\033]133;B\007"
}
__velowork_preexec() {
    printf "\033]133;C\007"
}
__velowork_precmd() {
    local exit_code=$?
    printf "\033]133;D;%d\007" "$exit_code"
    __velowork_prompt_start
}

if [[ -z "$__VELOWORK_SHELL_INTEGRATION_LOADED" ]]; then
    export __VELOWORK_SHELL_INTEGRATION_LOADED=1
    PROMPT_COMMAND="__velowork_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
    PS1="\[$(__velowork_prompt_end)\]$PS1"
    trap '__velowork_preexec' DEBUG
fi
"#;

/// Zsh shell integration snippet.
pub const ZSH_INTEGRATION_SCRIPT: &str = r#"
if [[ -z "$__VELOWORK_SHELL_INTEGRATION_LOADED" ]]; then
    export __VELOWORK_SHELL_INTEGRATION_LOADED=1

    __velowork_osc133_precmd() {
        local exit_code=$?
        printf "\033]133;D;%d\007" "$exit_code"
        printf "\033]133;A\007"
    }

    __velowork_osc133_preexec() {
        printf "\033]133;C\007"
    }

    autoload -Uz add-zsh-hook
    add-zsh-hook precmd __velowork_osc133_precmd
    add-zsh-hook preexec __velowork_osc133_preexec
    PS1="%{\033]133;B\007%}$PS1"
fi
"#;

/// Fish shell integration snippet.
pub const FISH_INTEGRATION_SCRIPT: &str = r#"
if not set -q __VELOWORK_SHELL_INTEGRATION_LOADED
    set -g __VELOWORK_SHELL_INTEGRATION_LOADED 1

    function __velowork_postexec --on-event fish_postexec
        printf "\033]133;D;%d\007" $status
    end

    function __velowork_preexec --on-event fish_preexec
        printf "\033]133;C\007"
    end

    functions -c fish_prompt __velowork_orig_fish_prompt
    function fish_prompt
        printf "\033]133;A\007"
        __velowork_orig_fish_prompt
        printf "\033]133;B\007"
    end
end
"#;

/// PowerShell integration snippet.
pub const PWSH_INTEGRATION_SCRIPT: &str = r#"
if (-not $env:__VELOWORK_SHELL_INTEGRATION_LOADED) {
    $env:__VELOWORK_SHELL_INTEGRATION_LOADED = "1"

    $origPrompt = $function:prompt
    function prompt {
        $lastExit = if ($?) { 0 } else { 1 }
        [Console]::Write("`e]133;D;$lastExit`a")
        [Console]::Write("`e]133;A`a")
        $p = & $origPrompt
        [Console]::Write("`e]133;B`a")
        return $p
    }
}
"#;

/// Sentinel token prefixes for non-OSC 133 command output boundary detection.
pub const SENTINEL_START_PREFIX: &str = "__VELO_CMD_START__:";
pub const SENTINEL_END_PREFIX: &str = "__VELO_CMD_END__:";

/// Wrap a command with sentinel echo markers for environments without shell integration.
///
/// Example output:
/// `echo "__VELO_CMD_START__:<token>"; cmd; echo "__VELO_CMD_END__:<token>:$?"`
pub fn wrap_command_with_sentinel(cmd: &str, token: &str) -> String {
    format!(
        "echo \"{start}{token}\"; {cmd}; echo \"{end}{token}:$?\"",
        start = SENTINEL_START_PREFIX,
        end = SENTINEL_END_PREFIX,
        token = token,
        cmd = cmd.trim_end()
    )
}

/// Result of parsing sentinel output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SentinelCapture {
    pub output: String,
    pub exit_code: Option<i32>,
}

/// Extract clean command output and exit code from raw text containing sentinel markers.
pub fn parse_sentinel_output(raw: &str, token: &str) -> Option<SentinelCapture> {
    let start_marker = format!("{}{}", SENTINEL_START_PREFIX, token);
    let end_marker = format!("{}{}:", SENTINEL_END_PREFIX, token);

    let start_idx = raw.find(&start_marker)?;
    let after_start = &raw[start_idx + start_marker.len()..];

    // Find end marker
    let end_idx = after_start.find(&end_marker)?;
    let output_slice = &after_start[..end_idx];

    // Parse exit code from after end_marker until newline or end
    let after_end = &after_start[end_idx + end_marker.len()..];
    let exit_code = after_end
        .lines()
        .next()
        .and_then(|line| line.trim().parse::<i32>().ok());

    // Normalize output: strip leading/trailing newlines
    let output = output_slice
        .trim_start_matches(|c| c == '\r' || c == '\n')
        .trim_end_matches(|c| c == '\r' || c == '\n')
        .to_string();

    Some(SentinelCapture { output, exit_code })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_command_with_sentinel() {
        let wrapped = wrap_command_with_sentinel("echo hello", "token123");
        assert!(wrapped.contains("__VELO_CMD_START__:token123"));
        assert!(wrapped.contains("echo hello"));
        assert!(wrapped.contains("__VELO_CMD_END__:token123:$?"));
    }

    #[test]
    fn test_parse_sentinel_output() {
        let raw = "\r\n__VELO_CMD_START__:abc456\r\nHello World\r\nLine 2\r\n__VELO_CMD_END__:abc456:0\r\n";
        let captured = parse_sentinel_output(raw, "abc456").expect("should parse");
        assert_eq!(captured.exit_code, Some(0));
        assert_eq!(captured.output, "Hello World\r\nLine 2");
    }

    #[test]
    fn test_parse_sentinel_output_failure_code() {
        let raw = "__VELO_CMD_START__:xyz\nerror: command not found\n__VELO_CMD_END__:xyz:127\n";
        let captured = parse_sentinel_output(raw, "xyz").expect("should parse");
        assert_eq!(captured.exit_code, Some(127));
        assert_eq!(captured.output, "error: command not found");
    }
}
