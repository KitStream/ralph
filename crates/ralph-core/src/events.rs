use serde::{Deserialize, Serialize};

use crate::session::state::{SessionStatus, SessionStep};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    pub session_id: String,
    pub payload: SessionEventPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SessionEventPayload {
    StatusChanged {
        status: SessionStatus,
    },
    Log {
        category: LogCategory,
        text: String,
    },
    /// Structured AI content block (tool use, tool result, or text).
    AiContent {
        block: AiContentBlock,
    },
    /// Structured housekeeping event (git operations, steps, recovery).
    Housekeeping {
        block: HousekeepingBlock,
    },
    IterationComplete {
        iteration: u32,
        tag: Option<String>,
    },
    Finished {
        reason: String,
    },
    /// The AI session ID changed (for crash recovery persistence).
    AiSessionIdChanged {
        ai_session_id: Option<String>,
    },
    /// Rate limited — session is paused until limit resets.
    RateLimited {
        message: String,
    },
    /// The session needs user input to proceed.
    ActionRequired {
        error: String,
        options: Vec<RecoveryOption>,
    },
}

/// A block of AI content, normalized across all providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AiContentBlock {
    /// Thinking / explanation text.
    Text { text: String },
    /// AI invoked a tool.
    ToolUse { tool_id: String, tool: ToolInvocation },
    /// Result returned from a tool invocation.
    ToolResult { tool_use_id: String, content: String, is_error: bool },
}

/// Canonical representation of tool invocations.
/// Each variant fully describes the tool input in a typed way.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool")]
pub enum ToolInvocation {
    Read { file_path: String },
    Edit { file_path: String, old_string: String, new_string: String },
    Write { file_path: String, content: String },
    Bash { command: String, description: Option<String> },
    Glob { pattern: String, path: Option<String> },
    Grep { pattern: String, path: Option<String>, include: Option<String> },
    /// Catch-all for tools we don't have specific rendering for.
    Other { name: String, input: serde_json::Value },
}

/// Structured housekeeping event for git operations and session lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum HousekeepingBlock {
    /// A named step started (checkout, rebase, push, tag).
    StepStarted { step: SessionStep, description: String },
    /// A step completed.
    StepCompleted { step: SessionStep, summary: String },
    /// Git command output.
    GitCommand { command: String, output: String, success: bool },
    /// Diff stat of pushed files.
    DiffStat { stat: String },
    /// Recovery action taken.
    Recovery { action: String, detail: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryOption {
    pub id: String,
    pub label: String,
    pub description: String,
}

/// User's chosen recovery action, sent back to the runner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryAction {
    Stash,
    Commit,
    HardReset,
    Abort,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogCategory {
    Git,
    Ai,
    Script,
    Warning,
    Error,
}

/// Callback type for emitting events. Both CLI and GUI provide their own implementation.
pub type EventCallback = Box<dyn Fn(SessionEvent) + Send + Sync>;
