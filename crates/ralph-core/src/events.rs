use serde::{Deserialize, Serialize};

use crate::session::state::SessionStatus;

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
    IterationComplete {
        iteration: u32,
        tag: Option<String>,
    },
    Finished {
        reason: String,
    },
    /// The session needs user input to proceed.
    ActionRequired {
        error: String,
        options: Vec<RecoveryOption>,
    },
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
