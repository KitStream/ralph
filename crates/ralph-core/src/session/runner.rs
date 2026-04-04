use std::sync::Arc;

use tokio::sync::{mpsc, watch};

use crate::events::{RecoveryAction, SessionEvent};
use crate::git::ops::GitOps;
use crate::provider::{create_provider, AiProvider};
use crate::session::machine::{SessionContext, SessionMachine};
use crate::session::state::{SessionConfig, SessionId, SessionStep};

/// Run the session iteration loop.
/// This is the core loop that both CLI and GUI call.
/// `emit` is called for each event (status changes, log lines, etc.).
pub async fn run_session(
    id: SessionId,
    config: SessionConfig,
    emit: impl Fn(SessionEvent) + Send + Sync + 'static,
    stop_rx: watch::Receiver<bool>,
    abort_rx: watch::Receiver<bool>,
    action_rx: mpsc::Receiver<RecoveryAction>,
    resume_ai_session_id: Option<String>,
    resume_step: Option<SessionStep>,
    resume_iteration: Option<u32>,
) {
    let git = GitOps::new(
        &config.project_dir,
        &config.branch_name,
        &config.main_branch,
    );
    let provider: Arc<dyn AiProvider> = Arc::from(create_provider(&config.ai_tool));

    let ctx = SessionContext {
        iteration: resume_iteration.unwrap_or(0),
        ai_session_id: resume_ai_session_id,
        stash_pending: false,
        head_before_ai: None,
        ai_attempts: 0,
        ai_ok: false,
        skip_to_step: resume_step,
        recovery_error: None,
        last_tag: None,
    };

    let mut machine = SessionMachine::new(
        id.to_string(),
        &config,
        &git,
        provider,
        &emit,
        stop_rx,
        abort_rx,
        action_rx,
        ctx,
    );

    machine.run().await;
}
