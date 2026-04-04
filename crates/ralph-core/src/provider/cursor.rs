use std::path::Path;
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

use super::{
    detect_rate_limit, parse_tool_invocation, AiOutput, AiProvider, BackendModelConfig, ModelInfo,
};

pub struct CursorProvider;

#[async_trait::async_trait]
impl AiProvider for CursorProvider {
    fn name(&self) -> &str {
        "Cursor"
    }

    async fn list_models(&self) -> BackendModelConfig {
        // Try to dynamically discover models via cursor-agent --list-models
        match Command::new("cursor-agent")
            .arg("--list-models")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut models = Vec::new();
                let mut current = None;
                for line in stdout.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with("Available") || line.starts_with("---") {
                        continue;
                    }
                    // Expected format: "id - Label  (default)" or "id - Label"
                    let is_default = line.contains("(default)") || line.contains("(current)");
                    let clean = line.replace("(default)", "").replace("(current)", "");
                    let parts: Vec<&str> = clean.splitn(2, " - ").collect();
                    let id = parts[0].trim().to_string();
                    let label = if parts.len() > 1 {
                        parts[1].trim().to_string()
                    } else {
                        id.clone()
                    };
                    if is_default {
                        current = Some(id.clone());
                    }
                    models.push(ModelInfo {
                        id,
                        label,
                        is_default,
                    });
                }
                BackendModelConfig {
                    models,
                    supports_freeform: false,
                    current_model: current,
                }
            }
            _ => {
                // CLI not available or failed — fallback to freeform
                BackendModelConfig {
                    models: vec![],
                    supports_freeform: true,
                    current_model: None,
                }
            }
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

        let mut cmd = Command::new("cursor-agent");
        cmd.arg("-p")
            .arg("--yolo")
            .arg("--output-format")
            .arg("stream-json");

        if let Some(m) = model {
            cmd.arg("--model").arg(m);
        }

        // Always pass --resume so we control the session ID for crash recovery.
        // If resuming, reuse the existing ID; otherwise generate a fresh one.
        let session_id = resume_session_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        cmd.arg("--resume").arg(&session_id);
        let _ = output_tx.send(AiOutput::SessionId(session_id));

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
            if detect_rate_limit(&stderr_buf) {
                let _ = output_tx.send(AiOutput::RateLimited {
                    message: stderr_buf.trim().to_string(),
                });
                return Ok(());
            }
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
        let event_type = value.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let subtype = value.get("subtype").and_then(|t| t.as_str()).unwrap_or("");

        match event_type {
            "assistant" => {
                // Text content in message.content[]
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
            "tool_call" if subtype == "started" => {
                let call_id = value
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if let Some(tc) = value.get("tool_call") {
                    // Shell tool call
                    if let Some(shell) = tc.get("shellToolCall") {
                        let command = shell
                            .get("args")
                            .and_then(|a| a.get("command"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let description = shell
                            .get("description")
                            .or_else(|| tc.get("description"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let tool = crate::events::ToolInvocation::Bash {
                            command,
                            description,
                        };
                        let _ = output_tx.send(AiOutput::ToolUse {
                            tool_id: call_id,
                            tool,
                        });
                    }
                    // Edit tool call — defer to completed event where we have the diff data
                    else if tc.get("editToolCall").is_some() {
                        // Don't emit on started; completed handler will emit with full diff
                    }
                    // Read tool call
                    else if let Some(read) = tc.get("readToolCall") {
                        let file_path = read
                            .get("args")
                            .and_then(|a| a.get("filePath").or_else(|| a.get("path")))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let tool = crate::events::ToolInvocation::Read { file_path };
                        let _ = output_tx.send(AiOutput::ToolUse {
                            tool_id: call_id,
                            tool,
                        });
                    }
                    // Generic fallback — discover tool type from the key name (e.g. "listDirToolCall")
                    else {
                        // Find the *ToolCall key to get the tool type name
                        let mut tool_name = String::from("tool");
                        let mut args = serde_json::Value::Null;
                        for (key, val) in tc.as_object().into_iter().flat_map(|m| m.iter()) {
                            if key.ends_with("ToolCall") || key.ends_with("_tool_call") {
                                tool_name = key
                                    .trim_end_matches("ToolCall")
                                    .trim_end_matches("_tool_call")
                                    .to_string();
                                args = val.get("args").cloned().unwrap_or(val.clone());
                                break;
                            }
                        }
                        // Try description as a fallback name
                        if tool_name == "tool" {
                            if let Some(desc) = tc.get("description").and_then(|v| v.as_str()) {
                                tool_name = desc.to_string();
                            }
                        }
                        // Map known cursor tool names to canonical names, then use parse_tool_invocation
                        let canonical_name = match tool_name.as_str() {
                            "listDir" | "list_dir" | "glob" | "find" => "Glob",
                            "grep" | "search" | "codebaseSearch" | "codebase_search" => "Grep",
                            "edit" | "write" | "create" => "Edit",
                            "read" | "view" => "Read",
                            "bash" | "terminal" | "shell" => "Bash",
                            other => other,
                        };
                        let tool = parse_tool_invocation(canonical_name, &args);
                        let _ = output_tx.send(AiOutput::ToolUse {
                            tool_id: call_id,
                            tool,
                        });
                    }
                }
            }
            "tool_call" if subtype == "completed" => {
                let call_id = value
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if let Some(tc) = value.get("tool_call") {
                    // Special handling for editToolCall — emit ToolUse with diff data
                    if let Some(edit) = tc.get("editToolCall") {
                        let file_path = edit
                            .get("args")
                            .and_then(|a| a.get("path").or_else(|| a.get("filePath")))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if let Some(success) = edit.get("result").and_then(|r| r.get("success")) {
                            let old_content = success
                                .get("beforeFullFileContent")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let new_content = success
                                .get("afterFullFileContent")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let tool = crate::events::ToolInvocation::Edit {
                                file_path,
                                old_string: old_content,
                                new_string: new_content,
                            };
                            let _ = output_tx.send(AiOutput::ToolUse {
                                tool_id: call_id.clone(),
                                tool,
                            });
                            let msg = success
                                .get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let _ = output_tx.send(AiOutput::ToolResult {
                                tool_use_id: call_id,
                                content: msg,
                                is_error: false,
                            });
                        } else if let Some(err) = edit.get("result").and_then(|r| r.get("error")) {
                            let tool = crate::events::ToolInvocation::Edit {
                                file_path,
                                old_string: String::new(),
                                new_string: String::new(),
                            };
                            let _ = output_tx.send(AiOutput::ToolUse {
                                tool_id: call_id.clone(),
                                tool,
                            });
                            let msg = err
                                .get("errorMessage")
                                .or_else(|| err.get("message"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("edit failed")
                                .to_string();
                            let _ = output_tx.send(AiOutput::ToolResult {
                                tool_use_id: call_id,
                                content: msg,
                                is_error: true,
                            });
                        }
                    } else {
                        // Find the tool-specific object (shellToolCall, readToolCall, etc.)
                        // and extract the result from it
                        let mut found = false;
                        for (key, tool_obj) in tc.as_object().into_iter().flat_map(|m| m.iter()) {
                            if !key.ends_with("ToolCall") && !key.ends_with("_tool_call") {
                                continue;
                            }
                            let Some(result) = tool_obj.get("result") else {
                                continue;
                            };
                            found = true;

                            if let Some(success) = result.get("success") {
                                // Shell tool: stdout/stderr/exitCode
                                if let Some(stdout) = success.get("stdout").and_then(|v| v.as_str())
                                {
                                    let stderr = success
                                        .get("stderr")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    let exit_code = success
                                        .get("exitCode")
                                        .and_then(|v| v.as_i64())
                                        .unwrap_or(0);
                                    let content = if stderr.is_empty() {
                                        stdout.to_string()
                                    } else {
                                        format!("{}\n{}", stdout, stderr)
                                    };
                                    let _ = output_tx.send(AiOutput::ToolResult {
                                        tool_use_id: call_id.clone(),
                                        content,
                                        is_error: exit_code != 0,
                                    });
                                }
                                // Read/other tools: content field, diffString, or message
                                else if let Some(content) = success
                                    .get("content")
                                    .or_else(|| success.get("diffString"))
                                    .or_else(|| success.get("message"))
                                    .and_then(|v| v.as_str())
                                {
                                    let _ = output_tx.send(AiOutput::ToolResult {
                                        tool_use_id: call_id.clone(),
                                        content: content.to_string(),
                                        is_error: false,
                                    });
                                }
                            } else if let Some(err) =
                                result.get("error").or_else(|| result.get("failure"))
                            {
                                let msg = err
                                    .get("stdout")
                                    .or_else(|| err.get("message"))
                                    .or_else(|| err.get("errorMessage"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("error");
                                let _ = output_tx.send(AiOutput::ToolResult {
                                    tool_use_id: call_id.clone(),
                                    content: msg.to_string(),
                                    is_error: true,
                                });
                            }
                            break;
                        }
                        if !found {
                            // Last resort: emit empty result so the UI doesn't hang
                            let _ = output_tx.send(AiOutput::ToolResult {
                                tool_use_id: call_id,
                                content: String::new(),
                                is_error: false,
                            });
                        }
                    } // end else (non-edit tools)
                }
            }
            "result" => {
                if let Some(sid) = value.get("session_id").and_then(|s| s.as_str()) {
                    let _ = output_tx.send(AiOutput::SessionId(sid.to_string()));
                }
            }
            _ => {}
        }
    } else if !line.trim().is_empty() {
        let _ = output_tx.send(AiOutput::Text(line.to_string()));
    }
}
