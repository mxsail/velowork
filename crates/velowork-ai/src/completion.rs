//! Ghost Text command completion and natural-language to command translation engine.

use serde_json::json;
use crate::context::TerminalContextSnapshot;
use crate::prompt::{render_inline_prompt, PromptScene};
use crate::provider::{stream_complete, LlmConfig};
use crate::runtime::AIError;

/// Clean raw LLM response into a single-line executable shell command.
///
/// Strips markdown fences, quotes, leading `$ `, and explanations.
pub fn clean_ghost_command(raw: &str) -> String {
    let trimmed = raw.trim();

    // 1. Strip markdown fences if present
    let content = if let Some(stripped) = trimmed.strip_prefix("```") {
        let after_lang_line = if let Some(nl_idx) = stripped.find('\n') {
            &stripped[nl_idx + 1..]
        } else {
            stripped
        };
        if let Some(end_idx) = after_lang_line.find("```") {
            &after_lang_line[..end_idx]
        } else {
            after_lang_line
        }
    } else {
        trimmed
    };

    // 2. Take the first non-empty line
    let first_line = content
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("");

    // 3. Strip leading prompts/quotes
    let mut cleaned = first_line.trim();
    if let Some(rest) = cleaned.strip_prefix("$ ") {
        cleaned = rest.trim();
    } else if let Some(rest) = cleaned.strip_prefix("# ") {
        cleaned = rest.trim();
    } else if let Some(rest) = cleaned.strip_prefix("> ") {
        cleaned = rest.trim();
    }

    cleaned
        .trim_matches(|c| c == '`' || c == '"' || c == '\'')
        .trim()
        .to_string()
}

/// Perform instant 0ms local command history matching before calling remote LLM.
pub fn fast_local_history_match(input: &str, history: &[String]) -> Option<String> {
    let query = input.trim();
    if query.is_empty() || query.starts_with('#') || query.starts_with('?') {
        return None;
    }

    // Match the most recent command starting with query (but not equal to query)
    history.iter().rev().find_map(|cmd| {
        let trimmed = cmd.trim();
        if trimmed.starts_with(query) && trimmed.len() > query.len() {
            Some(trimmed.to_string())
        } else {
            None
        }
    })
}

/// Asynchronously generate a ghost text command suggestion via LLM.
pub fn generate_ghost_command(
    input: &str,
    is_natural_language: bool,
    snapshot: &TerminalContextSnapshot,
    llm: &LlmConfig,
) -> Result<String, AIError> {
    // Defense: Sensitive password shielding
    let lower_input = input.to_lowercase();
    if lower_input.contains("password")
        || lower_input.contains("passwd")
        || lower_input.contains("secret")
        || lower_input.contains("passphrase")
    {
        return Err(AIError::Other {
            message: "sensitive input detected; ghost text disabled".to_string(),
        });
    }

    let scene = if is_natural_language {
        PromptScene::GhostTextCompletion
    } else {
        PromptScene::CommandCompletion
    };

    let system_prompt = render_inline_prompt(scene, snapshot);
    let messages = vec![
        json!({ "role": "system", "content": system_prompt }),
        json!({ "role": "user", "content": input }),
    ];

    let mut raw_output = String::new();
    stream_complete(
        &llm.base_url,
        &llm.api_key,
        &llm.model_id,
        &messages,
        |delta| {
            raw_output.push_str(delta);
        },
    )?;

    let cleaned = clean_ghost_command(&raw_output);
    if cleaned.is_empty() {
        Err(AIError::Other {
            message: "empty suggestion generated".to_string(),
        })
    } else {
        Ok(cleaned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_ghost_command_plain() {
        assert_eq!(clean_ghost_command("ls -la"), "ls -la");
        assert_eq!(clean_ghost_command("  $ ps aux | grep node  "), "ps aux | grep node");
        assert_eq!(clean_ghost_command("`git status`"), "git status");
    }

    #[test]
    fn test_clean_ghost_command_markdown() {
        let raw = "```bash\nfree -m\n```\nHere is the command to check memory.";
        assert_eq!(clean_ghost_command(raw), "free -m");

        let raw_sh = "```sh\ndocker ps -a\n```";
        assert_eq!(clean_ghost_command(raw_sh), "docker ps -a");
    }

    #[test]
    fn test_fast_local_history_match() {
        let history = vec![
            "cargo check".to_string(),
            "git status".to_string(),
            "docker ps -a".to_string(),
            "git checkout main".to_string(),
        ];

        assert_eq!(
            fast_local_history_match("git c", &history),
            Some("git checkout main".to_string())
        );
        assert_eq!(
            fast_local_history_match("doc", &history),
            Some("docker ps -a".to_string())
        );
        assert_eq!(fast_local_history_match("git checkout main", &history), None);
        assert_eq!(fast_local_history_match("# list files", &history), None);
    }
}
