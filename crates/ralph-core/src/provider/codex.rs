use std::path::Path;

use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

use crate::events::ToolInvocation;

use super::{AiOutput, AiProvider};

pub struct CodexProvider;

#[async_trait::async_trait]
impl AiProvider for CodexProvider {
    fn name(&self) -> &str {
        "Codex"
    }

    async fn run(
        &self,
        working_dir: &Path,
        prompt: &str,
        resume_session_id: Option<&str>,
        output_tx: mpsc::UnboundedSender<AiOutput>,
        mut abort: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let mut cmd = Command::new("codex");

        if let Some(id) = resume_session_id {
            // codex exec resume <thread-id> <prompt> --json ...
            cmd.args(["exec", "resume"])
                .arg(id)
                .arg(prompt)
                .arg("--json")
                .arg("--dangerously-bypass-approvals-and-sandbox");
        } else {
            cmd.args(["exec", "--dangerously-bypass-approvals-and-sandbox"])
                .arg(prompt)
                .arg("--json");
        }

        let mut child = cmd
            .current_dir(working_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn codex: {}", e))?;

        let stdout = child.stdout.take().unwrap();
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();

        loop {
            tokio::select! {
                line = lines.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            parse_codex_json_line(&line, &output_tx);
                        }
                        Ok(None) => break,
                        Err(e) => {
                            let _ = output_tx.send(AiOutput::Error(format!("Read error: {}", e)));
                            break;
                        }
                    }
                }
                _ = abort.changed() => {
                    if *abort.borrow() {
                        child.kill().await.ok();
                        return Ok(());
                    }
                }
            }
        }

        let status = child.wait().await?;
        let _ = output_tx.send(AiOutput::Finished {
            duration_secs: 0.0,
            cost_usd: None,
        });

        if !status.success() {
            anyhow::bail!("Codex exited with non-zero status");
        }
        Ok(())
    }
}

fn parse_codex_json_line(line: &str, output_tx: &mpsc::UnboundedSender<AiOutput>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        if !line.trim().is_empty() {
            let _ = output_tx.send(AiOutput::Text(line.to_string()));
        }
        return;
    };

    match value.get("type").and_then(|t| t.as_str()) {
        Some("thread.started") => {
            if let Some(tid) = value.get("thread_id").and_then(|t| t.as_str()) {
                let _ = output_tx.send(AiOutput::SessionId(tid.to_string()));
            }
        }
        Some("item.completed") | Some("item.started") => {
            if let Some(item) = value.get("item") {
                let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match item_type {
                    "command_execution" => {
                        let tool_id = item
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let command = item
                            .get("command")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let status = item
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        if status == "in_progress" {
                            // item.started — emit tool use
                            let tool = ToolInvocation::Bash {
                                command,
                                description: None,
                            };
                            let _ = output_tx.send(AiOutput::ToolUse { tool_id, tool });
                        } else if status == "completed" {
                            // item.completed — emit tool result
                            let output = item
                                .get("aggregated_output")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let exit_code = item
                                .get("exit_code")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0);
                            let _ = output_tx.send(AiOutput::ToolResult {
                                tool_use_id: tool_id,
                                content: output,
                                is_error: exit_code != 0,
                            });
                        }
                    }
                    "agent_message" => {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            if !text.trim().is_empty() {
                                let _ = output_tx.send(AiOutput::Text(text.to_string()));
                            }
                        }
                    }
                    _ => {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            if !text.trim().is_empty() {
                                let _ = output_tx.send(AiOutput::Text(text.to_string()));
                            }
                        }
                    }
                }
            }
        }
        _ => {
            // Fallback: try common fields
            if let Some(text) = value.get("text").and_then(|t| t.as_str()) {
                if !text.trim().is_empty() {
                    let _ = output_tx.send(AiOutput::Text(text.to_string()));
                }
            }
        }
    }
}
