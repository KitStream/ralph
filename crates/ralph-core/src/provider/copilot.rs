use std::path::{Path, PathBuf};
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

use super::{detect_rate_limit, parse_tool_invocation, AiOutput, AiProvider, BackendModelConfig, ModelInfo};

fn copilot_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".copilot").join("config.json"))
}

fn read_copilot_current_model() -> Option<String> {
    let path = copilot_config_path()?;
    let data = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&data).ok()?;
    json.get("model").and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn copilot_known_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo { id: "claude-sonnet-4.6".into(), label: "Claude Sonnet 4.6".into(), is_default: false },
        ModelInfo { id: "claude-opus-4.6".into(), label: "Claude Opus 4.6".into(), is_default: false },
        ModelInfo { id: "gpt-5.2".into(), label: "GPT-5.2".into(), is_default: false },
        ModelInfo { id: "gpt-5-mini".into(), label: "GPT-5 mini".into(), is_default: false },
        ModelInfo { id: "gpt-4.1".into(), label: "GPT-4.1".into(), is_default: false },
    ]
}

pub struct CopilotProvider;

#[async_trait::async_trait]
impl AiProvider for CopilotProvider {
    fn name(&self) -> &str {
        "Copilot"
    }

    async fn list_models(&self) -> BackendModelConfig {
        let current = read_copilot_current_model();
        let mut models = copilot_known_models();

        // Mark the current model as default if found in the list
        let matched = if let Some(cur) = &current {
            models.iter_mut().any(|m| {
                if m.id == *cur {
                    m.is_default = true;
                    true
                } else {
                    false
                }
            })
        } else {
            false
        };
        if !matched {
            if let Some(m) = models.first_mut() {
                m.is_default = true;
            }
        }

        BackendModelConfig {
            current_model: current,
            models,
            supports_freeform: true,
        }
    }

    async fn run(
        &self,
        working_dir: &Path,
        prompt: &str,
        model: Option<&str>,
        resume_session_id: Option<&str>,
        output_tx: mpsc::UnboundedSender<AiOutput>,
        mut abort: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let start = Instant::now();

        let mut cmd = Command::new("copilot");
        cmd.arg("-p")
            .arg(prompt)
            .arg("--allow-all")
            .arg("--output-format")
            .arg("json");

        if let Some(m) = model {
            cmd.arg("--model").arg(m);
        }

        if let Some(id) = resume_session_id {
            cmd.arg(format!("--resume={}", id));
        }

        let mut child = cmd
            .current_dir(working_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn copilot: {}", e))?;

        let stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();

        loop {
            tokio::select! {
                line = lines.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            parse_copilot_json_line(&line, &output_tx);
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
                        let _ = output_tx.send(AiOutput::Error("Aborted".to_string()));
                        return Ok(());
                    }
                }
            }
        }

        let mut stderr_buf = String::new();
        stderr.read_to_string(&mut stderr_buf).await.ok();

        let status = child.wait().await?;
        let duration = start.elapsed().as_secs_f64();

        let _ = output_tx.send(AiOutput::Finished {
            duration_secs: duration,
            cost_usd: None,
        });

        if !status.success() {
            if detect_rate_limit(&stderr_buf) {
                let _ = output_tx.send(AiOutput::RateLimited {
                    message: stderr_buf.trim().to_string(),
                });
                return Ok(());
            }
            let err_msg = if stderr_buf.trim().is_empty() {
                format!("Copilot exited with code {}", status.code().unwrap_or(-1))
            } else {
                format!("Copilot exited with code {}: {}", status.code().unwrap_or(-1), stderr_buf.trim())
            };
            let _ = output_tx.send(AiOutput::Error(err_msg.clone()));
            anyhow::bail!("{}", err_msg);
        }
        Ok(())
    }
}

fn parse_copilot_json_line(line: &str, output_tx: &mpsc::UnboundedSender<AiOutput>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        if !line.trim().is_empty() {
            let _ = output_tx.send(AiOutput::Text(line.to_string()));
        }
        return;
    };

    match value.get("type").and_then(|t| t.as_str()) {
        Some("assistant.message_delta") => {
            // Streaming text delta
            if let Some(text) = value
                .get("data")
                .and_then(|d| d.get("deltaContent"))
                .and_then(|c| c.as_str())
            {
                if !text.trim().is_empty() {
                    let _ = output_tx.send(AiOutput::Text(text.to_string()));
                }
            }
        }
        Some("assistant.message") => {
            // Full message — text already received via message_delta events,
            // tool requests will come via tool.execution_start events.
            // Nothing to emit here.
        }
        Some("tool.execution_start") => {
            // Tool invocation started — already emitted from assistant.message toolRequests,
            // but we can use this as a fallback
            if let Some(data) = value.get("data") {
                let tool_id = data
                    .get("toolCallId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_name = data
                    .get("toolName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let input = data
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let tool = parse_tool_invocation(tool_name, &input);
                let _ = output_tx.send(AiOutput::ToolUse { tool_id, tool });
            }
        }
        Some("tool.execution_complete") => {
            // Tool result
            if let Some(data) = value.get("data") {
                let tool_use_id = data
                    .get("toolCallId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let success = data
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let content = data
                    .get("result")
                    .and_then(|r| r.get("content"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let _ = output_tx.send(AiOutput::ToolResult {
                    tool_use_id,
                    content,
                    is_error: !success,
                });
            }
        }
        Some("result") => {
            if let Some(sid) = value.get("sessionId").and_then(|s| s.as_str()) {
                let _ = output_tx.send(AiOutput::SessionId(sid.to_string()));
            }
        }
        _ => {}
    }
}
