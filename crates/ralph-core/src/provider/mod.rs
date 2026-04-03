pub mod claude;
pub mod codex;
pub mod copilot;
pub mod cursor;

use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use crate::events::ToolInvocation;

/// Output from an AI tool process.
#[derive(Debug, Clone)]
pub enum AiOutput {
    /// Text content to display.
    Text(String),
    /// AI invoked a tool (normalized).
    ToolUse {
        tool_id: String,
        tool: ToolInvocation,
    },
    /// Result returned from a tool invocation.
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    /// Rate limit hit — the AI provider wants us to wait.
    RateLimited {
        message: String,
    },
    /// Execution finished with summary.
    Finished {
        duration_secs: f64,
        cost_usd: Option<f64>,
    },
    /// Process exited with error.
    Error(String),
    /// The AI backend's session ID (for crash recovery resume).
    SessionId(String),
}

/// Parse raw tool name + JSON input into a canonical ToolInvocation.
/// All providers should call this to normalize their output.
pub fn parse_tool_invocation(name: &str, input: &serde_json::Value) -> ToolInvocation {
    let str_field = |field: &str| -> String {
        input
            .get(field)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let opt_str_field = |field: &str| -> Option<String> {
        input
            .get(field)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };

    match name {
        "Read" => ToolInvocation::Read {
            file_path: str_field("file_path"),
        },
        "Edit" => ToolInvocation::Edit {
            file_path: str_field("file_path"),
            old_string: str_field("old_string"),
            new_string: str_field("new_string"),
        },
        "Write" => ToolInvocation::Write {
            file_path: str_field("file_path"),
            content: str_field("content"),
        },
        "Bash" => ToolInvocation::Bash {
            command: str_field("command"),
            description: opt_str_field("description"),
        },
        "Glob" => ToolInvocation::Glob {
            pattern: str_field("pattern"),
            path: opt_str_field("path"),
        },
        "Grep" => ToolInvocation::Grep {
            pattern: str_field("pattern"),
            path: opt_str_field("path"),
            include: opt_str_field("include"),
        },
        _ => ToolInvocation::Other {
            name: name.to_string(),
            input: input.clone(),
        },
    }
}

/// Check if text indicates a rate limit. Returns the message if it does.
pub fn detect_rate_limit(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("hit your limit")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("quota exceeded")
        || lower.contains("usage limit")
        || lower.contains("token limit")
}

/// Which AI tool to use.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AiTool {
    Claude,
    Codex,
    Copilot,
    Cursor,
}

impl std::fmt::Display for AiTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiTool::Claude => write!(f, "claude"),
            AiTool::Codex => write!(f, "codex"),
            AiTool::Copilot => write!(f, "copilot"),
            AiTool::Cursor => write!(f, "cursor"),
        }
    }
}

impl std::str::FromStr for AiTool {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "claude" => Ok(AiTool::Claude),
            "codex" => Ok(AiTool::Codex),
            "copilot" => Ok(AiTool::Copilot),
            "cursor" => Ok(AiTool::Cursor),
            _ => Err(format!("Unknown AI tool: {}", s)),
        }
    }
}

/// Trait for AI coding tool providers.
#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    /// Human-readable name.
    fn name(&self) -> &str;

    /// Run the AI tool in the given working directory with the given prompt.
    /// Streams output into `output_tx`.
    /// Returns Ok(()) on success, Err on failure.
    async fn run(
        &self,
        working_dir: &Path,
        prompt: &str,
        resume_session_id: Option<&str>,
        output_tx: mpsc::UnboundedSender<AiOutput>,
        abort: watch::Receiver<bool>,
    ) -> anyhow::Result<()>;
}

/// Create a provider for the given tool.
pub fn create_provider(tool: &AiTool) -> Box<dyn AiProvider> {
    match tool {
        AiTool::Claude => Box::new(claude::ClaudeProvider),
        AiTool::Codex => Box::new(codex::CodexProvider),
        AiTool::Copilot => Box::new(copilot::CopilotProvider),
        AiTool::Cursor => Box::new(cursor::CursorProvider),
    }
}
