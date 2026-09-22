use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::{Workflow, WorkflowStep};

/// Central registry managing available workflow templates.
#[derive(Clone)]
pub struct WorkflowRegistry {
    workflows: Arc<RwLock<HashMap<String, Workflow>>>,
}

impl Default for WorkflowRegistry {
    fn default() -> Self {
        let registry = Self {
            workflows: Arc::new(RwLock::new(HashMap::new())),
        };
        registry.register_builtins();
        registry
    }
}

impl WorkflowRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register built-in standard developer & DevOps workflows.
    fn register_builtins(&self) {
        // 1. Service Health Check Pipeline
        let health_check = Workflow {
            id: "service_health_check".to_string(),
            name: "Service & Port Health Check".to_string(),
            description: "Inspect network port listening, process status, and service vitality".to_string(),
            steps: vec![
                WorkflowStep::Command {
                    name: "Check Port Listeners".to_string(),
                    cmd: "ss -tuln 2>/dev/null || netstat -tuln 2>/dev/null || echo 'network tools unavailable'".to_string(),
                    target_terminal_id: None,
                    expect_exit_code: None,
                    timeout_secs: Some(10),
                },
                WorkflowStep::Command {
                    name: "Check Active Processes".to_string(),
                    cmd: "ps -eo pid,ppid,%cpu,%mem,comm --sort=-%cpu | head -n 12".to_string(),
                    target_terminal_id: None,
                    expect_exit_code: None,
                    timeout_secs: Some(10),
                },
                WorkflowStep::AiDecision {
                    name: "Synthesize Health Status".to_string(),
                    prompt_template: "Analyze the following system status and identify port conflicts or high resource processes:\n```\n{{last_output}}\n```".to_string(),
                },
            ],
            max_total_timeout_secs: Some(60),
        };
        self.register(health_check);

        // 2. Git Safe Pre-commit Inspection
        let git_audit = Workflow {
            id: "git_safe_precommit".to_string(),
            name: "Git Safe Pre-commit Inspection".to_string(),
            description: "Audit working tree status, staged diffs, and untracked files before committing".to_string(),
            steps: vec![
                WorkflowStep::Command {
                    name: "Check Git Status".to_string(),
                    cmd: "git status -s".to_string(),
                    target_terminal_id: None,
                    expect_exit_code: Some(0),
                    timeout_secs: Some(10),
                },
                WorkflowStep::Command {
                    name: "Check Git Diff Summary".to_string(),
                    cmd: "git diff --stat".to_string(),
                    target_terminal_id: None,
                    expect_exit_code: Some(0),
                    timeout_secs: Some(10),
                },
                WorkflowStep::UserConfirmation {
                    name: "Confirm Working Tree".to_string(),
                    warning_message: "Please review the modified files listed above before proceeding.".to_string(),
                    suggested_action: "Proceed to commit if changes look correct.".to_string(),
                },
            ],
            max_total_timeout_secs: Some(60),
        };
        self.register(git_audit);

        // 3. System Quick Diagnostics
        let diagnostics = Workflow {
            id: "quick_diagnostics".to_string(),
            name: "System Quick Diagnostics".to_string(),
            description: "Collect OS load, uptime, memory, and disk usage for rapid troubleshooting".to_string(),
            steps: vec![
                WorkflowStep::Command {
                    name: "OS & Uptime".to_string(),
                    cmd: "uname -a && uptime".to_string(),
                    target_terminal_id: None,
                    expect_exit_code: Some(0),
                    timeout_secs: Some(5),
                },
                WorkflowStep::Command {
                    name: "Disk Space".to_string(),
                    cmd: "df -h".to_string(),
                    target_terminal_id: None,
                    expect_exit_code: Some(0),
                    timeout_secs: Some(5),
                },
                WorkflowStep::AiDecision {
                    name: "Diagnose System State".to_string(),
                    prompt_template: "Summarize disk and load metrics, highlighting any threshold alerts:\n```\n{{last_output}}\n```".to_string(),
                },
            ],
            max_total_timeout_secs: Some(30),
        };
        self.register(diagnostics);
    }

    /// Register a new or custom workflow.
    pub fn register(&self, workflow: Workflow) {
        if let Ok(mut lock) = self.workflows.write() {
            lock.insert(workflow.id.clone(), workflow);
        }
    }

    /// Retrieve a workflow by its unique ID.
    pub fn get(&self, id: &str) -> Option<Workflow> {
        self.workflows.read().ok().and_then(|w| w.get(id).cloned())
    }

    /// List all registered workflows.
    pub fn list(&self) -> Vec<Workflow> {
        self.workflows
            .read()
            .map(|w| w.values().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_builtins_loaded() {
        let reg = WorkflowRegistry::default();
        let list = reg.list();
        assert!(list.len() >= 3);
        assert!(reg.get("service_health_check").is_some());
        assert!(reg.get("git_safe_precommit").is_some());
        assert!(reg.get("quick_diagnostics").is_some());
    }

    #[test]
    fn test_register_custom_workflow() {
        let reg = WorkflowRegistry::default();
        let custom = Workflow {
            id: "my_custom".to_string(),
            name: "My Custom".to_string(),
            description: "Custom test".to_string(),
            steps: vec![],
            max_total_timeout_secs: None,
        };
        reg.register(custom);
        assert!(reg.get("my_custom").is_some());
    }
}
