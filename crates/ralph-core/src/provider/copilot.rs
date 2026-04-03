use std::path::Path;
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

use super::{detect_rate_limit, parse_tool_invocation, AiOutput, AiProvider};

pub struct CopilotProvider;

#[async_trait::async_trait]
impl AiProvider for CopilotProvider {
    fn name(&self) -> &str {
        "Copilot"
    }

    async fn run(
        &self,
        working_dir: &Path,
        prompt: &str,
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
        Some("assistant.tool_use") | Some("tool_use") => {
            let tool_id = value
                .get("id")
                .or_else(|| value.get("data").and_then(|d| d.get("id")))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let tool_name = value
                .get("name")
                .or_else(|| value.get("data").and_then(|d| d.get("name")))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let input = value
                .get("input")
                .or_else(|| value.get("data").and_then(|d| d.get("input")))
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let tool = parse_tool_invocation(tool_name, &input);
            let _ = output_tx.send(AiOutput::ToolUse { tool_id, tool });
        }
        Some("tool_result") => {
            let tool_use_id = value
                .get("tool_use_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let is_error = value
                .get("is_error")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let content = value
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let _ = output_tx.send(AiOutput::ToolResult {
                tool_use_id,
                content,
                is_error,
            });
        }
        Some("result") => {
            if let Some(sid) = value.get("sessionId").and_then(|s| s.as_str()) {
                let _ = output_tx.send(AiOutput::SessionId(sid.to_string()));
            }
        }
        _ => {}
    }
}
