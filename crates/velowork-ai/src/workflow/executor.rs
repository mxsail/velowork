//! Workflow execution engine with step evaluation, safety guards, and output capture.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{ConditionCheck, StepExecutionResult, Workflow, WorkflowStep};
use crate::context::{head_tail_truncate, mask_sensitive_data, strip_ansi};

/// Current lifecycle state of a workflow execution.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", content = "detail")]
pub enum WorkflowState {
    Pending,
    Running {
        current_step: usize,
        total_steps: usize,
        step_name: String,
    },
    WaitingForConfirmation {
        step_index: usize,
        warning_message: String,
        suggested_action: String,
    },
    Completed {
        duration_ms: u64,
        total_steps_executed: usize,
    },
    Failed {
        failed_step: usize,
        step_name: String,
        error: String,
    },
    Aborted,
}

/// Structured summary report of a workflow execution.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkflowExecutionReport {
    pub workflow_id: String,
    pub workflow_name: String,
    pub state: WorkflowState,
    pub step_results: Vec<StepExecutionResult>,
    pub total_duration_ms: u64,
    pub summary: String,
}

/// Pluggable command execution closure signature.
///
/// Signature: `(command, target_terminal_id, timeout) -> Result<(exit_code, output), error_message>`
pub type CommandRunner =
    Arc<dyn Fn(&str, Option<&str>, Duration) -> Result<(Option<i32>, String), String> + Send + Sync>;

/// Default command runner executing via system shell.
pub fn default_system_command_runner() -> CommandRunner {
    Arc::new(|cmd: &str, _target_term: Option<&str>, timeout: Duration| {
        let start = Instant::now();

        #[cfg(target_os = "windows")]
        let mut child = std::process::Command::new("cmd")
            .args(["/C", cmd])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to spawn cmd process: {e}"))?;

        #[cfg(not(target_os = "windows"))]
        let mut child = std::process::Command::new("sh")
            .args(["-c", cmd])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to spawn sh process: {e}"))?;

        // Poll with timeout
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut stdout = Vec::new();
                    let mut stderr = Vec::new();
                    if let Some(mut out) = child.stdout.take() {
                        use std::io::Read;
                        let _ = out.read_to_end(&mut stdout);
                    }
                    if let Some(mut err) = child.stderr.take() {
                        use std::io::Read;
                        let _ = err.read_to_end(&mut stderr);
                    }
                    let combined = format!(
                        "{}{}",
                        String::from_utf8_lossy(&stdout),
                        String::from_utf8_lossy(&stderr)
                    );
                    return Ok((status.code(), combined));
                }
                Ok(None) => {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        return Err(format!("command timed out after {}s", timeout.as_secs()));
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(format!("error waiting for process: {e}")),
            }
        }
    })
}

/// Orchestrator for executing structured workflows.
pub struct WorkflowExecutor {
    runner: CommandRunner,
    cancel_token: Arc<AtomicBool>,
    auto_confirm: bool,
}

impl Default for WorkflowExecutor {
    fn default() -> Self {
        Self {
            runner: default_system_command_runner(),
            cancel_token: Arc::new(AtomicBool::new(false)),
            auto_confirm: false,
        }
    }
}

impl WorkflowExecutor {
    pub fn new(runner: CommandRunner) -> Self {
        Self {
            runner,
            cancel_token: Arc::new(AtomicBool::new(false)),
            auto_confirm: false,
        }
    }

    /// Set whether user confirmation steps should be automatically approved.
    pub fn with_auto_confirm(mut self, auto: bool) -> Self {
        self.auto_confirm = auto;
        self
    }

    /// Obtain a shared cancellation handle.
    pub fn cancel_token(&self) -> Arc<AtomicBool> {
        self.cancel_token.clone()
    }

    /// Request cancellation of the running workflow.
    pub fn cancel(&self) {
        self.cancel_token.store(true, Ordering::SeqCst);
    }

    /// Execute the complete workflow and return a structured execution report.
    pub fn execute(&self, workflow: &Workflow) -> WorkflowExecutionReport {
        let start_time = Instant::now();
        let mut step_results: Vec<StepExecutionResult> = Vec::new();
        let total_timeout = workflow
            .max_total_timeout_secs
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(600));

        let res = self.execute_steps(
            &workflow.steps,
            &mut step_results,
            start_time,
            total_timeout,
        );

        let total_duration_ms = start_time.elapsed().as_millis() as u64;

        let (state, summary) = match res {
            Ok(total_executed) => (
                WorkflowState::Completed {
                    duration_ms: total_duration_ms,
                    total_steps_executed: total_executed,
                },
                format!(
                    "Workflow '{}' successfully completed ({} steps executed in {:.2}s)",
                    workflow.name,
                    total_executed,
                    total_duration_ms as f64 / 1000.0
                ),
            ),
            Err(WorkflowExecutionError::Aborted) => (
                WorkflowState::Aborted,
                format!("Workflow '{}' was aborted by user", workflow.name),
            ),
            Err(WorkflowExecutionError::WaitingForConfirmation {
                step_index,
                warning,
                action,
            }) => (
                WorkflowState::WaitingForConfirmation {
                    step_index,
                    warning_message: warning.clone(),
                    suggested_action: action,
                },
                format!("Workflow paused at step {step_index}: {warning}"),
            ),
            Err(WorkflowExecutionError::Failed {
                step_index,
                step_name,
                message,
            }) => (
                WorkflowState::Failed {
                    failed_step: step_index,
                    step_name: step_name.clone(),
                    error: message.clone(),
                },
                format!("Workflow failed at step {step_index} ('{step_name}'): {message}"),
            ),
        };

        WorkflowExecutionReport {
            workflow_id: workflow.id.clone(),
            workflow_name: workflow.name.clone(),
            state,
            step_results,
            total_duration_ms,
            summary,
        }
    }

    fn execute_steps(
        &self,
        steps: &[WorkflowStep],
        results: &mut Vec<StepExecutionResult>,
        start_time: Instant,
        total_timeout: Duration,
    ) -> Result<usize, WorkflowExecutionError> {
        let mut executed_count = 0;

        for step in steps {
            // Check cancellation
            if self.cancel_token.load(Ordering::Relaxed) {
                return Err(WorkflowExecutionError::Aborted);
            }

            // Check overall timeout
            if start_time.elapsed() > total_timeout {
                return Err(WorkflowExecutionError::Failed {
                    step_index: results.len(),
                    step_name: step.name().to_string(),
                    message: format!("Workflow exceeded total timeout of {}s", total_timeout.as_secs()),
                });
            }

            let step_start = Instant::now();
            match step {
                WorkflowStep::Command {
                    name,
                    cmd,
                    target_terminal_id,
                    expect_exit_code,
                    timeout_secs,
                } => {
                    let timeout = timeout_secs
                        .map(Duration::from_secs)
                        .unwrap_or(Duration::from_secs(60));

                    let run_res = (self.runner)(cmd, target_terminal_id.as_deref(), timeout);
                    let duration_ms = step_start.elapsed().as_millis() as u64;

                    match run_res {
                        Ok((code, raw_output)) => {
                            let cleaned = strip_ansi(&raw_output);
                            let masked = mask_sensitive_data(&cleaned);
                            let truncated = head_tail_truncate(&masked, 4000, 1500, 2000);

                            let success = match expect_exit_code {
                                Some(expected) => code == Some(*expected),
                                None => code.unwrap_or(0) == 0,
                            };

                            let error_msg = if !success {
                                Some(format!(
                                    "exit code {:?} did not match expected {:?}",
                                    code, expect_exit_code
                                ))
                            } else {
                                None
                            };

                            results.push(StepExecutionResult {
                                step_name: name.clone(),
                                success,
                                exit_code: code,
                                output_preview: truncated,
                                duration_ms,
                                error: error_msg.clone(),
                            });
                            executed_count += 1;

                            if !success {
                                return Err(WorkflowExecutionError::Failed {
                                    step_index: results.len() - 1,
                                    step_name: name.clone(),
                                    message: error_msg.unwrap_or_else(|| "command failed".into()),
                                });
                            }
                        }
                        Err(err) => {
                            results.push(StepExecutionResult {
                                step_name: name.clone(),
                                success: false,
                                exit_code: None,
                                output_preview: String::new(),
                                duration_ms,
                                error: Some(err.clone()),
                            });
                            return Err(WorkflowExecutionError::Failed {
                                step_index: results.len() - 1,
                                step_name: name.clone(),
                                message: err,
                            });
                        }
                    }
                }
                WorkflowStep::Condition {
                    name,
                    check,
                    on_success,
                    on_failure,
                } => {
                    let last_result = results.last();
                    let cond_met = match check {
                        ConditionCheck::LastExitCodeIs(expected) => {
                            last_result.and_then(|r| r.exit_code) == Some(*expected)
                        }
                        ConditionCheck::LastOutputContains(needle) => {
                            last_result.map(|r| r.output_preview.contains(needle)).unwrap_or(false)
                        }
                        ConditionCheck::LastOutputNotContains(needle) => {
                            last_result.map(|r| !r.output_preview.contains(needle)).unwrap_or(true)
                        }
                        ConditionCheck::LastOutputMatchesRegex(pattern) => {
                            if let Ok(re) = regex::Regex::new(pattern) {
                                last_result.map(|r| re.is_match(&r.output_preview)).unwrap_or(false)
                            } else {
                                false
                            }
                        }
                    };

                    let duration_ms = step_start.elapsed().as_millis() as u64;
                    results.push(StepExecutionResult {
                        step_name: name.clone(),
                        success: true,
                        exit_code: None,
                        output_preview: format!("Condition evaluated to: {cond_met}"),
                        duration_ms,
                        error: None,
                    });
                    executed_count += 1;

                    let sub_branch = if cond_met { on_success } else { on_failure };
                    let sub_count = self.execute_steps(
                        sub_branch,
                        results,
                        start_time,
                        total_timeout,
                    )?;
                    executed_count += sub_count;
                }
                WorkflowStep::AiDecision {
                    name,
                    prompt_template,
                } => {
                    // Record AI decision evaluation node
                    let duration_ms = step_start.elapsed().as_millis() as u64;
                    let last_out = results.last().map(|r| r.output_preview.as_str()).unwrap_or("");
                    let prompt = prompt_template.replace("{{last_output}}", last_out);

                    results.push(StepExecutionResult {
                        step_name: name.clone(),
                        success: true,
                        exit_code: Some(0),
                        output_preview: format!("AI Decision Prepared: {prompt}"),
                        duration_ms,
                        error: None,
                    });
                    executed_count += 1;
                }
                WorkflowStep::UserConfirmation {
                    name,
                    warning_message,
                    suggested_action,
                } => {
                    let duration_ms = step_start.elapsed().as_millis() as u64;
                    if self.auto_confirm {
                        results.push(StepExecutionResult {
                            step_name: name.clone(),
                            success: true,
                            exit_code: Some(0),
                            output_preview: format!("Auto-confirmed: {warning_message}"),
                            duration_ms,
                            error: None,
                        });
                        executed_count += 1;
                    } else {
                        results.push(StepExecutionResult {
                            step_name: name.clone(),
                            success: false,
                            exit_code: None,
                            output_preview: format!("Awaiting confirmation: {warning_message}"),
                            duration_ms,
                            error: None,
                        });
                        return Err(WorkflowExecutionError::WaitingForConfirmation {
                            step_index: results.len() - 1,
                            warning: warning_message.clone(),
                            action: suggested_action.clone(),
                        });
                    }
                }
            }
        }

        Ok(executed_count)
    }
}

enum WorkflowExecutionError {
    Aborted,
    WaitingForConfirmation {
        step_index: usize,
        warning: String,
        action: String,
    },
    Failed {
        step_index: usize,
        step_name: String,
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_successful_multi_step_workflow() {
        let runner: CommandRunner = Arc::new(|cmd, _, _| {
            match cmd {
                "step1" => Ok((Some(0), "step1 done".into())),
                "step2" => Ok((Some(0), "step2 done".into())),
                _ => Err("unknown command".into()),
            }
        });

        let executor = WorkflowExecutor::new(runner);
        let wf = Workflow {
            id: "wf-test-success".into(),
            name: "Test Success".into(),
            description: "Success test".into(),
            steps: vec![
                WorkflowStep::Command {
                    name: "Step 1".into(),
                    cmd: "step1".into(),
                    target_terminal_id: None,
                    expect_exit_code: Some(0),
                    timeout_secs: Some(5),
                },
                WorkflowStep::Command {
                    name: "Step 2".into(),
                    cmd: "step2".into(),
                    target_terminal_id: None,
                    expect_exit_code: Some(0),
                    timeout_secs: Some(5),
                },
            ],
            max_total_timeout_secs: Some(10),
        };

        let report = executor.execute(&wf);
        assert!(matches!(report.state, WorkflowState::Completed { total_steps_executed: 2, .. }));
        assert_eq!(report.step_results.len(), 2);
        assert_eq!(report.step_results[0].output_preview, "step1 done");
        assert_eq!(report.step_results[1].output_preview, "step2 done");
    }

    #[test]
    fn test_condition_branching() {
        let runner: CommandRunner = Arc::new(|cmd, _, _| {
            match cmd {
                "check" => Ok((Some(0), "PORT 8080 OPEN".into())),
                "on_open" => Ok((Some(0), "connected to 8080".into())),
                _ => Err("failed".into()),
            }
        });

        let executor = WorkflowExecutor::new(runner);
        let wf = Workflow {
            id: "wf-cond".into(),
            name: "Condition Test".into(),
            description: "Branching test".into(),
            steps: vec![
                WorkflowStep::Command {
                    name: "Check Port".into(),
                    cmd: "check".into(),
                    target_terminal_id: None,
                    expect_exit_code: Some(0),
                    timeout_secs: Some(5),
                },
                WorkflowStep::Condition {
                    name: "Port 8080 open?".into(),
                    check: ConditionCheck::LastOutputContains("8080 OPEN".into()),
                    on_success: vec![WorkflowStep::Command {
                        name: "Handle Open".into(),
                        cmd: "on_open".into(),
                        target_terminal_id: None,
                        expect_exit_code: Some(0),
                        timeout_secs: Some(5),
                    }],
                    on_failure: vec![],
                },
            ],
            max_total_timeout_secs: Some(10),
        };

        let report = executor.execute(&wf);
        assert!(matches!(report.state, WorkflowState::Completed { total_steps_executed: 3, .. }));
        assert_eq!(report.step_results[2].step_name, "Handle Open");
    }

    #[test]
    fn test_workflow_cancellation() {
        let runner: CommandRunner = Arc::new(|_, _, _| Ok((Some(0), "done".into())));
        let executor = WorkflowExecutor::new(runner);
        executor.cancel();

        let wf = Workflow {
            id: "wf-cancel".into(),
            name: "Cancel Test".into(),
            description: "test".into(),
            steps: vec![WorkflowStep::Command {
                name: "Step".into(),
                cmd: "echo".into(),
                target_terminal_id: None,
                expect_exit_code: Some(0),
                timeout_secs: Some(5),
            }],
            max_total_timeout_secs: Some(10),
        };

        let report = executor.execute(&wf);
        assert_eq!(report.state, WorkflowState::Aborted);
    }
}
