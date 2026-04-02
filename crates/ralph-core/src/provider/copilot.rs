use std::path::Path;

use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

use super::{AiOutput, AiProvider};

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
        output_tx: mpsc::UnboundedSender<AiOutput>,
        mut abort: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let mut child = Command::new("copilot")
            .arg("-p")
            .arg(prompt)
            .arg("--allow-all")
            .current_dir(working_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn copilot: {}", e))?;

        let stdout = child.stdout.take().unwrap();
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();

        loop {
            tokio::select! {
                line = lines.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            if !line.trim().is_empty() {
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
            anyhow::bail!("Copilot exited with non-zero status");
        }
        Ok(())
    }
}
