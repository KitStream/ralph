use std::path::{Path, PathBuf};
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

use crate::events::ToolInvocation;

use super::{
    detect_rate_limit, parse_tool_invocation, AiOutput, AiProvider, BackendModelConfig, ModelInfo,
};

fn copilot_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".copilot").join("config.json"))
}

fn read_copilot_current_model() -> Option<String> {
    let path = copilot_config_path()?;
    let data = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&data).ok()?;
    json.get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn copilot_known_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "claude-opus-4.7".into(),
            label: "Claude Opus 4.7".into(),
            is_default: false,
        },
        ModelInfo {
            id: "claude-sonnet-4.6".into(),
            label: "Claude Sonnet 4.6".into(),
            is_default: false,
        },
        ModelInfo {
            id: "claude-opus-4.6".into(),
            label: "Claude Opus 4.6".into(),
            is_default: false,
        },
        ModelInfo {
            id: "gpt-5-5".into(),
            label: "GPT-5.5".into(),
            is_default: false,
        },
        ModelInfo {
            id: "gpt-5.2".into(),
            label: "GPT-5.2".into(),
            is_default: false,
        },
        ModelInfo {
            id: "gpt-5-mini".into(),
            label: "GPT-5 mini".into(),
            is_default: false,
        },
        ModelInfo {
            id: "gpt-4.1".into(),
            label: "GPT-4.1".into(),
            is_default: false,
        },
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

        let mut cmd = Command::new(super::resolve_tool_command("copilot", "copilot"));
        cmd.arg("-p")
            .arg(prompt)
            .arg("--allow-all")
            .arg("--output-format")
            .arg("json");

        if let Some(m) = model {
            cmd.arg("--model").arg(m);
        }

        // Always pass --resume so we control the session ID for crash recovery.
        // If resuming, reuse the existing ID; otherwise generate a fresh one.
        let session_id = resume_session_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        cmd.arg(format!("--resume={}", session_id));
        let _ = output_tx.send(AiOutput::SessionId(session_id));

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
        let mut text_buf = String::new();

        loop {
            tokio::select! {
                line = lines.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            parse_copilot_json_line(&line, &output_tx, &mut text_buf);
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

        // Flush any remaining accumulated text
        if !text_buf.is_empty() {
            let _ = output_tx.send(AiOutput::Text(std::mem::take(&mut text_buf)));
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
                format!(
                    "Copilot exited with code {}: {}",
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

fn flush_text_buf(text_buf: &mut String, output_tx: &mpsc::UnboundedSender<AiOutput>) {
    if !text_buf.is_empty() {
        let _ = output_tx.send(AiOutput::Text(std::mem::take(text_buf)));
    }
}

/// Convert a Copilot tool invocation into one or more `AiOutput::ToolUse`
/// events. Most tools map 1:1, but `apply_patch` is expanded into one Edit
/// (or Write) per file in the patch so the renderer can show each change
/// as a structured diff rather than a raw list of patch lines.
fn emit_tool_use(
    tool_name: &str,
    tool_id: &str,
    input: &serde_json::Value,
    output_tx: &mpsc::UnboundedSender<AiOutput>,
) {
    if tool_name == "apply_patch" {
        if let Some(text) = extract_patch_text(input) {
            let ops = parse_apply_patch(&text);
            if !ops.is_empty() {
                for op in ops {
                    let _ = output_tx.send(AiOutput::ToolUse {
                        tool_id: tool_id.to_string(),
                        tool: op.into_tool_invocation(),
                    });
                }
                return;
            }
        }
    }
    let tool = parse_tool_invocation(tool_name, input);
    let _ = output_tx.send(AiOutput::ToolUse {
        tool_id: tool_id.to_string(),
        tool,
    });
}

/// Extract the raw patch text from an apply_patch arguments object. The exact
/// field name varies between Copilot CLI versions, so try the known aliases
/// before giving up.
fn extract_patch_text(input: &serde_json::Value) -> Option<String> {
    for key in ["input", "patch", "diff", "patch_text"] {
        if let Some(s) = input.get(key).and_then(|v| v.as_str()) {
            if !s.trim().is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

#[derive(Debug, PartialEq)]
enum PatchOp {
    Update {
        path: String,
        old_text: String,
        new_text: String,
    },
    Add {
        path: String,
        content: String,
    },
    Delete {
        path: String,
    },
}

impl PatchOp {
    fn into_tool_invocation(self) -> ToolInvocation {
        match self {
            PatchOp::Update {
                path,
                old_text,
                new_text,
            } => ToolInvocation::Edit {
                file_path: path,
                old_string: old_text,
                new_string: new_text,
            },
            PatchOp::Add { path, content } => ToolInvocation::Write {
                file_path: path,
                content,
            },
            PatchOp::Delete { path } => ToolInvocation::Other {
                name: "Delete".to_string(),
                input: serde_json::json!({ "path": path }),
            },
        }
    }
}

/// Parse the standard `apply_patch` envelope into structured per-file ops.
/// Tolerates missing `*** Begin Patch` / `*** End Patch` markers and treats
/// hunk headers (`@@ ...`) as separators rather than content.
fn parse_apply_patch(input: &str) -> Vec<PatchOp> {
    let mut ops: Vec<PatchOp> = Vec::new();
    let mut current: Option<PatchOp> = None;

    let push = |ops: &mut Vec<PatchOp>, op: Option<PatchOp>| {
        if let Some(op) = op {
            ops.push(op);
        }
    };

    for line in input.lines() {
        if let Some(rest) = line.strip_prefix("*** Update File:") {
            push(&mut ops, current.take());
            current = Some(PatchOp::Update {
                path: rest.trim().to_string(),
                old_text: String::new(),
                new_text: String::new(),
            });
        } else if let Some(rest) = line.strip_prefix("*** Add File:") {
            push(&mut ops, current.take());
            current = Some(PatchOp::Add {
                path: rest.trim().to_string(),
                content: String::new(),
            });
        } else if let Some(rest) = line.strip_prefix("*** Delete File:") {
            push(&mut ops, current.take());
            ops.push(PatchOp::Delete {
                path: rest.trim().to_string(),
            });
        } else if line == "*** Begin Patch" || line == "*** End Patch" {
            // markers — no content
        } else if line.starts_with("@@") {
            // hunk header — separator, no content
        } else {
            match current.as_mut() {
                Some(PatchOp::Update {
                    old_text, new_text, ..
                }) => {
                    if let Some(rest) = line.strip_prefix('-') {
                        old_text.push_str(rest);
                        old_text.push('\n');
                    } else if let Some(rest) = line.strip_prefix('+') {
                        new_text.push_str(rest);
                        new_text.push('\n');
                    } else {
                        // Context line — present in both old and new sides.
                        // The leading space marker is optional in the wild;
                        // strip it if present, otherwise take the line as-is.
                        let ctx = line.strip_prefix(' ').unwrap_or(line);
                        old_text.push_str(ctx);
                        old_text.push('\n');
                        new_text.push_str(ctx);
                        new_text.push('\n');
                    }
                }
                Some(PatchOp::Add { content, .. }) => {
                    let ctx = line.strip_prefix('+').unwrap_or(line);
                    content.push_str(ctx);
                    content.push('\n');
                }
                _ => {}
            }
        }
    }
    push(&mut ops, current.take());

    for op in ops.iter_mut() {
        match op {
            PatchOp::Update {
                old_text, new_text, ..
            } => {
                trim_trailing_newline(old_text);
                trim_trailing_newline(new_text);
            }
            PatchOp::Add { content, .. } => {
                trim_trailing_newline(content);
            }
            _ => {}
        }
    }
    ops
}

fn trim_trailing_newline(s: &mut String) {
    if s.ends_with('\n') {
        s.pop();
    }
}

fn parse_copilot_json_line(
    line: &str,
    output_tx: &mpsc::UnboundedSender<AiOutput>,
    text_buf: &mut String,
) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        if !line.trim().is_empty() {
            text_buf.push_str(line);
        }
        return;
    };

    match value.get("type").and_then(|t| t.as_str()) {
        Some("assistant.message_delta") => {
            // Streaming text delta — accumulate instead of emitting immediately
            if let Some(text) = value
                .get("data")
                .and_then(|d| d.get("deltaContent"))
                .and_then(|c| c.as_str())
            {
                if !text.trim().is_empty() {
                    text_buf.push_str(text);
                }
            }
        }
        Some("assistant.message") => {
            // Full message complete — flush accumulated text
            flush_text_buf(text_buf, output_tx);
        }
        Some("tool.execution_start") => {
            // Flush text before tool events
            flush_text_buf(text_buf, output_tx);
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
                emit_tool_use(tool_name, &tool_id, &input, output_tx);
            }
        }
        Some("tool.execution_complete") => {
            flush_text_buf(text_buf, output_tx);
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
            flush_text_buf(text_buf, output_tx);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_apply_patch_single_update_hunk() {
        let patch = "*** Begin Patch\n\
            *** Update File: src/main.rs\n\
            @@ context\n\
             unchanged\n\
            -old line\n\
            +new line\n\
            *** End Patch";
        let ops = parse_apply_patch(patch);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PatchOp::Update {
                path,
                old_text,
                new_text,
            } => {
                assert_eq!(path, "src/main.rs");
                assert_eq!(old_text, "unchanged\nold line");
                assert_eq!(new_text, "unchanged\nnew line");
            }
            other => panic!("expected Update, got {:?}", other),
        }
    }

    #[test]
    fn parse_apply_patch_multiple_files() {
        let patch = "*** Begin Patch\n\
            *** Update File: a.rs\n\
            -alpha\n\
            +ALPHA\n\
            *** Add File: b.rs\n\
            +line1\n\
            +line2\n\
            *** Delete File: c.rs\n\
            *** End Patch";
        let ops = parse_apply_patch(patch);
        assert_eq!(ops.len(), 3);
        assert!(matches!(&ops[0], PatchOp::Update { path, .. } if path == "a.rs"));
        match &ops[1] {
            PatchOp::Add { path, content } => {
                assert_eq!(path, "b.rs");
                assert_eq!(content, "line1\nline2");
            }
            other => panic!("expected Add, got {:?}", other),
        }
        assert!(matches!(&ops[2], PatchOp::Delete { path } if path == "c.rs"));
    }

    #[test]
    fn parse_apply_patch_without_envelope_markers() {
        // Some emitters omit Begin/End markers — still parseable.
        let patch = "*** Update File: file.txt\n\
            -old\n\
            +new\n";
        let ops = parse_apply_patch(patch);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PatchOp::Update {
                old_text, new_text, ..
            } => {
                assert_eq!(old_text, "old");
                assert_eq!(new_text, "new");
            }
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn parse_apply_patch_skips_hunk_headers() {
        let patch = "*** Update File: file.txt\n\
            @@ -1,3 +1,3 @@\n\
             ctx\n\
            -a\n\
            +b\n";
        let ops = parse_apply_patch(patch);
        match &ops[0] {
            PatchOp::Update {
                old_text, new_text, ..
            } => {
                assert_eq!(old_text, "ctx\na");
                assert_eq!(new_text, "ctx\nb");
            }
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn extract_patch_text_tries_multiple_field_names() {
        let v = serde_json::json!({"input": "*** Begin Patch\n*** End Patch"});
        assert!(extract_patch_text(&v).is_some());
        let v = serde_json::json!({"patch": "*** Begin Patch\n*** End Patch"});
        assert!(extract_patch_text(&v).is_some());
        let v = serde_json::json!({"unknown_field": "x"});
        assert!(extract_patch_text(&v).is_none());
    }

    #[test]
    fn emit_tool_use_apply_patch_emits_one_per_file() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let input = serde_json::json!({
            "input": "*** Begin Patch\n\
                      *** Update File: a.rs\n\
                      -x\n\
                      +y\n\
                      *** Add File: b.rs\n\
                      +new\n\
                      *** End Patch"
        });
        emit_tool_use("apply_patch", "tool-1", &input, &tx);
        drop(tx);

        let mut tools = Vec::new();
        while let Ok(out) = rx.try_recv() {
            if let AiOutput::ToolUse { tool, .. } = out {
                tools.push(tool);
            }
        }
        assert_eq!(tools.len(), 2);
        assert!(matches!(&tools[0], ToolInvocation::Edit { file_path, .. } if file_path == "a.rs"));
        assert!(
            matches!(&tools[1], ToolInvocation::Write { file_path, .. } if file_path == "b.rs")
        );
    }

    #[test]
    fn emit_tool_use_apply_patch_falls_back_to_other_when_unparseable() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let input = serde_json::json!({"unknown": "garbage"});
        emit_tool_use("apply_patch", "tool-2", &input, &tx);
        drop(tx);
        let out = rx.try_recv().expect("expected one tool use");
        match out {
            AiOutput::ToolUse { tool, .. } => {
                assert!(matches!(tool, ToolInvocation::Other { .. }));
            }
            _ => panic!("expected ToolUse"),
        }
    }
}
