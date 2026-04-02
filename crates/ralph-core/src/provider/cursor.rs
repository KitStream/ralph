use std::path::Path;
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

use super::{AiOutput, AiProvider};

pub struct CursorProvider;

#[async_trait::async_trait]
impl AiProvider for CursorProvider {
    fn name(&self) -> &str {
        "Cursor"
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

        let mut cmd = Command::new("cursor-agent");
        cmd.arg("-p")
            .arg("--yolo")
            .arg("--output-format")
            .arg("stream-json");

        if let Some(id) = resume_session_id {
            cmd.arg("--resume").arg(id);
        }

        cmd.arg(prompt);

        let mut child = cmd
            .current_dir(working_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn cursor-agent: {}", e))?;

        let stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();

        loop {
            tokio::select! {
                line = lines.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            parse_cursor_line(&line, &output_tx);
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

        // Read stderr
        let mut stderr_buf = String::new();
        stderr.read_to_string(&mut stderr_buf).await.ok();

        let status = child.wait().await?;
        let duration = start.elapsed().as_secs_f64();

        let _ = output_tx.send(AiOutput::Finished {
            duration_secs: duration,
            cost_usd: None,
        });

        if !status.success() {
            let err_msg = if stderr_buf.trim().is_empty() {
                format!("Cursor exited with code {}", status.code().unwrap_or(-1))
            } else {
                format!(
                    "Cursor exited with code {}: {}",
                    status.code().unwrap_or(-1),
                    stderr_buf.trim()
                )
            };
            let _ = output_tx.send(AiOutput::Error(err_msg.clone()));
            anyhow::bail!("{}", err_msg);
        }
        Ok(())
    }
}

fn emit_non_empty(output_tx: &mpsc::UnboundedSender<AiOutput>, text: &str) {
    if !text.trim().is_empty() {
        let _ = output_tx.send(AiOutput::Text(text.to_string()));
    }
}

fn parse_cursor_line(line: &str, output_tx: &mpsc::UnboundedSender<AiOutput>) {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
        // Extract session_id from result events
        if let Some(sid) = value.get("session_id").and_then(|s| s.as_str()) {
            let _ = output_tx.send(AiOutput::SessionId(sid.to_string()));
        }

        // stream-json format: look for text content in various shapes
        if let Some(text) = value.get("text").and_then(|t| t.as_str()) {
            emit_non_empty(output_tx, text);
        } else if let Some(msg) = value.get("message").and_then(|m| m.as_str()) {
            emit_non_empty(output_tx, msg);
        } else if let Some(content) = value.get("content").and_then(|c| c.as_str()) {
            emit_non_empty(output_tx, content);
        } else if value.get("type").and_then(|t| t.as_str()) == Some("assistant") {
            // Same structure as Claude's stream-json
            if let Some(content) = value
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for item in content {
                    if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            emit_non_empty(output_tx, text);
                        }
                    }
                }
            }
        }
        // Silently ignore JSON lines that don't contain displayable text
        // (status events, tool calls, etc.)
    } else if !line.trim().is_empty() {
        // Plain text fallback
        let _ = output_tx.send(AiOutput::Text(line.to_string()));
    }
}
