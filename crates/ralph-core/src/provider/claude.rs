use std::path::Path;

use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

use super::{AiOutput, AiProvider};

pub struct ClaudeProvider;

#[async_trait::async_trait]
impl AiProvider for ClaudeProvider {
    fn name(&self) -> &str {
        "Claude"
    }

    async fn run(
        &self,
        working_dir: &Path,
        prompt: &str,
        resume_session_id: Option<&str>,
        output_tx: mpsc::UnboundedSender<AiOutput>,
        mut abort: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let mut cmd = Command::new("claude");
        cmd.arg("-p")
            .arg(prompt)
            .arg("--dangerously-skip-permissions")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose");

        if let Some(id) = resume_session_id {
            cmd.arg("--resume").arg(id);
        }

        let mut child = cmd
            .current_dir(working_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn claude: {}", e))?;

        let stdout = child.stdout.take().unwrap();
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();

        let mut got_result = false;
        loop {
            tokio::select! {
                line = lines.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            if parse_claude_json_line(&line, &output_tx) {
                                got_result = true;
                                break;
                            }
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

        // If we got the result but the process is still running, give it a moment
        // then kill it — Claude can linger after emitting the result JSON.
        if got_result {
            match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                child.wait(),
            )
            .await
            {
                Ok(Ok(status)) if status.success() => return Ok(()),
                Ok(Ok(status)) => {
                    let _ = output_tx.send(AiOutput::Error(format!(
                        "Claude exited with code {}",
                        status.code().unwrap_or(-1)
                    )));
                    anyhow::bail!("Claude exited with non-zero status");
                }
                _ => {
                    // Timed out or error — kill the lingering process
                    child.kill().await.ok();
                    return Ok(());
                }
            }
        }

        let status = child.wait().await?;
        if !status.success() {
            let _ = output_tx.send(AiOutput::Error(format!(
                "Claude exited with code {}",
                status.code().unwrap_or(-1)
            )));
            anyhow::bail!("Claude exited with non-zero status");
        }

        Ok(())
    }
}

/// Parse a JSON line from Claude's stream output.
/// Returns `true` if this was a "result" message (i.e. Claude is done).
fn parse_claude_json_line(line: &str, output_tx: &mpsc::UnboundedSender<AiOutput>) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        // Not JSON, emit as raw text
        if !line.trim().is_empty() {
            let _ = output_tx.send(AiOutput::Text(line.to_string()));
        }
        return false;
    };

    match value.get("type").and_then(|t| t.as_str()) {
        Some("assistant") => {
            if let Some(content) = value
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for item in content {
                    if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            let _ = output_tx.send(AiOutput::Text(text.to_string()));
                        }
                    }
                }
            }
            false
        }
        Some("result") => {
            if let Some(sid) = value.get("session_id").and_then(|s| s.as_str()) {
                let _ = output_tx.send(AiOutput::SessionId(sid.to_string()));
            }
            let duration_ms = value
                .get("duration_ms")
                .and_then(|d| d.as_f64())
                .unwrap_or(0.0);
            let cost_usd = value.get("total_cost_usd").and_then(|c| c.as_f64());
            let _ = output_tx.send(AiOutput::Finished {
                duration_secs: duration_ms / 1000.0,
                cost_usd,
            });
            true
        }
        _ => false,
    }
}
