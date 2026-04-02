pub mod claude;
pub mod codex;
pub mod copilot;
pub mod cursor;

use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

/// Output from an AI tool process.
#[derive(Debug, Clone)]
pub enum AiOutput {
    /// Text content to display.
    Text(String),
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
