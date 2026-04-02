use std::path::Path;

use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

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
        output_tx: mpsc::UnboundedSender<AiOutput>,
        mut abort: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let mut child = Command::new("codex")
            .args(["exec", "--ask-for-approval", "never"])
            .arg(prompt)
            .arg("--json")
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
                            // Codex JSON output — extract text content
                            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                                if let Some(text) = value.get("text").and_then(|t| t.as_str()) {
                                    let _ = output_tx.send(AiOutput::Text(text.to_string()));
                                } else if let Some(msg) = value.get("message").and_then(|m| m.as_str()) {
                                    let _ = output_tx.send(AiOutput::Text(msg.to_string()));
                                }
                            } else if !line.trim().is_empty() {
                                let _ = output_tx.send(AiOutput::Text(line));
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
