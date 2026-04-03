use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::provider::AiTool;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub project_dir: PathBuf,
    pub mode: String,
    pub prompt_file: PathBuf,
    pub branch_name: String,
    pub main_branch: String,
    pub preamble: String,
    pub tagging_enabled: bool,
    pub ai_tool: AiTool,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionStep {
    Idle,
    Checkout,
    RebasePreAi,
    RunningAi,
    PushBranch,
    RebasePostAi,
    PushToMain,
    Tagging,
    RecoveringGit,
    Paused,
}

impl std::fmt::Display for SessionStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionStep::Idle => write!(f, "Idle"),
            SessionStep::Checkout => write!(f, "Checkout"),
            SessionStep::RebasePreAi => write!(f, "Rebasing (pre-AI)"),
            SessionStep::RunningAi => write!(f, "Running AI"),
            SessionStep::PushBranch => write!(f, "Pushing branch"),
            SessionStep::RebasePostAi => write!(f, "Rebasing (post-AI)"),
            SessionStep::PushToMain => write!(f, "Pushing to main"),
            SessionStep::Tagging => write!(f, "Tagging"),
            SessionStep::RecoveringGit => write!(f, "Recovering git state"),
            SessionStep::Paused => write!(f, "Paused (rate limited)"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
    Created,
    Running {
        step: SessionStep,
        iteration: u32,
    },
    Stopping {
        step: SessionStep,
        iteration: u32,
    },
    Stopped,
    Aborted {
        ai_session_id: Option<String>,
        #[serde(default)]
        step: Option<SessionStep>,
        #[serde(default)]
        iteration: Option<u32>,
    },
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: SessionId,
    pub config: SessionConfig,
    pub status: SessionStatus,
    pub last_tag: Option<String>,
    pub iteration_count: u32,
    #[serde(default)]
    pub ai_session_id: Option<String>,
}
