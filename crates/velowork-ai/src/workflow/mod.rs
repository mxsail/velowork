//! Multi-step workflow definitions and data structures.
//!
//! Provides composable, automated pipelines for multi-step terminal tasks,
//! self-healing diagnostics, and safe agent execution.

use serde::{Deserialize, Serialize};

pub mod executor;
pub mod registry;

pub use executor::{WorkflowExecutionReport, WorkflowExecutor, WorkflowState};
pub use registry::WorkflowRegistry;

/// A multi-step automated workflow definition.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
    #[serde(default)]
    pub max_total_timeout_secs: Option<u64>,
}

/// A single step in an automated workflow.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "config")]
pub enum WorkflowStep {
    /// Execute a terminal/shell command and capture exit code & output.
    Command {
        name: String,
        cmd: String,
        #[serde(default)]
        target_terminal_id: Option<String>,
        #[serde(default)]
        expect_exit_code: Option<i32>,
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
    /// Evaluate a condition against previous step results.
    Condition {
        name: String,
        check: ConditionCheck,
        on_success: Vec<WorkflowStep>,
        #[serde(default)]
        on_failure: Vec<WorkflowStep>,
    },
    /// Artificial intelligence decision or summarization step.
    AiDecision {
        name: String,
        prompt_template: String,
    },
    /// Require explicit user confirmation before proceeding.
    UserConfirmation {
        name: String,
        warning_message: String,
        suggested_action: String,
    },
}

impl WorkflowStep {
    pub fn name(&self) -> &str {
        match self {
            Self::Command { name, .. } => name,
            Self::Condition { name, .. } => name,
            Self::AiDecision { name, .. } => name,
            Self::UserConfirmation { name, .. } => name,
        }
    }
}

/// Conditional assertion for workflow branching.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "check_type", content = "value")]
pub enum ConditionCheck {
    LastExitCodeIs(i32),
    LastOutputContains(String),
    LastOutputNotContains(String),
    LastOutputMatchesRegex(String),
}

/// Execution outcome of an individual workflow step.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StepExecutionResult {
    pub step_name: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub output_preview: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_serialization_roundtrip() {
        let wf = Workflow {
            id: "test-pipeline".to_string(),
            name: "Test Pipeline".to_string(),
            description: "CI check".to_string(),
            steps: vec![
                WorkflowStep::Command {
                    name: "Check syntax".to_string(),
                    cmd: "cargo check".to_string(),
                    target_terminal_id: None,
                    expect_exit_code: Some(0),
                    timeout_secs: Some(30),
                },
                WorkflowStep::Condition {
                    name: "Evaluate check".to_string(),
                    check: ConditionCheck::LastExitCodeIs(0),
                    on_success: vec![WorkflowStep::Command {
                        name: "Run tests".to_string(),
                        cmd: "cargo test".to_string(),
                        target_terminal_id: None,
                        expect_exit_code: Some(0),
                        timeout_secs: Some(60),
                    }],
                    on_failure: vec![WorkflowStep::UserConfirmation {
                        name: "Confirm failure".to_string(),
                        warning_message: "Compilation failed".to_string(),
                        suggested_action: "Fix code".to_string(),
                    }],
                },
            ],
            max_total_timeout_secs: Some(120),
        };

        let json_str = serde_json::to_string_pretty(&wf).expect("serialize workflow");
        let deserialized: Workflow = serde_json::from_str(&json_str).expect("deserialize workflow");
        assert_eq!(wf, deserialized);
    }
}
