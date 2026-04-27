use std::sync::Arc;

use tokio::sync::{mpsc, watch};

use crate::discovery::load_prompt;
use crate::events::{
    AiContentBlock, HousekeepingBlock, LogCategory, RecoveryAction, RecoveryOption, SessionEvent,
    SessionEventPayload,
};
use crate::git::ops::{GitOperations, RebaseError};
use crate::provider::{AiOutput, AiProvider};
use crate::session::state::{SessionConfig, SessionStatus, SessionStep};

const MAX_AI_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, PartialEq)]
pub enum IterationState {
    Setup,
    NewIteration,
    Checkout,
    RebasePreAi,
    StashPop,
    RunningAi,
    AiRetry,
    RateLimitPause,
    PushBranch,
    RebasePostAi,
    PushToMain,
    Tagging,
    IterationComplete,
    WaitingForRecovery,
    Done,
    Failed,
}

pub struct SessionContext {
    pub iteration: u32,
    pub ai_session_id: Option<String>,
    pub stash_pending: bool,
    pub head_before_ai: Option<String>,
    pub ai_attempts: u32,
    pub ai_ok: bool,
    pub skip_to_step: Option<SessionStep>,
    pub recovery_error: Option<String>,
    pub last_tag: Option<String>,
}

pub struct SessionMachine<'a> {
    session_id: String,
    config: &'a SessionConfig,
    git: &'a dyn GitOperations,
    provider: Arc<dyn AiProvider>,
    emit: &'a (dyn Fn(SessionEvent) + Send + Sync),
    stop_rx: watch::Receiver<bool>,
    abort_rx: watch::Receiver<bool>,
    action_rx: mpsc::Receiver<RecoveryAction>,
    ctx: SessionContext,
}

impl<'a> SessionMachine<'a> {
    pub fn new(
        session_id: String,
        config: &'a SessionConfig,
        git: &'a dyn GitOperations,
        provider: Arc<dyn AiProvider>,
        emit: &'a (dyn Fn(SessionEvent) + Send + Sync),
        stop_rx: watch::Receiver<bool>,
        abort_rx: watch::Receiver<bool>,
        action_rx: mpsc::Receiver<RecoveryAction>,
        ctx: SessionContext,
    ) -> Self {
        Self {
            session_id,
            config,
            git,
            provider,
            emit,
            stop_rx,
            abort_rx,
            action_rx,
            ctx,
        }
    }

    pub async fn run(&mut self) {
        let mut state = IterationState::Setup;
        while state != IterationState::Done && state != IterationState::Failed {
            state = self.transition(state).await;
        }
        if state == IterationState::Done && !*self.abort_rx.borrow() {
            self.emit_status(SessionStatus::Stopped);
            self.emit_event(SessionEventPayload::Finished {
                reason: "Stopped".to_string(),
            });
        }
    }

    async fn transition(&mut self, state: IterationState) -> IterationState {
        if *self.abort_rx.borrow() {
            return IterationState::Done;
        }
        match state {
            IterationState::Setup => self.do_setup().await,
            IterationState::NewIteration => self.do_new_iteration(),
            IterationState::Checkout => self.do_checkout().await,
            IterationState::RebasePreAi => self.do_rebase_pre_ai().await,
            IterationState::StashPop => self.do_stash_pop().await,
            IterationState::RunningAi => self.do_running_ai().await,
            IterationState::AiRetry => IterationState::RunningAi,
            IterationState::RateLimitPause => self.do_rate_limit_pause().await,
            IterationState::PushBranch => self.do_push_branch().await,
            IterationState::RebasePostAi => self.do_rebase_post_ai().await,
            IterationState::PushToMain => self.do_push_to_main().await,
            IterationState::Tagging => self.do_tagging().await,
            IterationState::IterationComplete => self.do_iteration_complete(),
            IterationState::WaitingForRecovery => self.do_waiting_for_recovery().await,
            IterationState::Done | IterationState::Failed => unreachable!(),
        }
    }

    // ── State handlers ──────────────────────────────────────────────────

    async fn do_setup(&mut self) -> IterationState {
        self.emit_log(
            LogCategory::Script,
            "Setting up branch and worktree...".to_string(),
        );

        if let Err(e) = self.git.ensure_branch_exists().await {
            self.emit_status(SessionStatus::Failed {
                error: format!("Failed to ensure branch: {}", e),
            });
            return IterationState::Failed;
        }

        if let Err(e) = self.git.ensure_worktree().await {
            self.emit_status(SessionStatus::Failed {
                error: format!("Failed to ensure worktree: {}", e),
            });
            return IterationState::Failed;
        }

        self.emit_log(
            LogCategory::Script,
            format!(
                "Running in worktree: {:?} (mode: {}, branch: {})",
                self.git.worktree_dir(),
                self.config.mode,
                self.config.branch_name
            ),
        );

        IterationState::NewIteration
    }

    fn do_new_iteration(&mut self) -> IterationState {
        if *self.stop_rx.borrow() {
            self.emit_log(LogCategory::Script, "Stop requested. Exiting.".to_string());
            return IterationState::Done;
        }

        // Reset per-iteration state
        self.ctx.ai_attempts = 0;
        self.ctx.ai_ok = false;
        self.ctx.head_before_ai = None;
        self.ctx.last_tag = None;

        if let Some(step) = self.ctx.skip_to_step.take() {
            self.emit_log(
                LogCategory::Script,
                format!("Resuming at {:?} (iteration {})", step, self.ctx.iteration),
            );
            let skip_pre_ai = matches!(
                step,
                SessionStep::RunningAi
                    | SessionStep::PushBranch
                    | SessionStep::RebasePostAi
                    | SessionStep::PushToMain
                    | SessionStep::Tagging
                    | SessionStep::Paused
            );
            if skip_pre_ai {
                self.emit_log(
                    LogCategory::Script,
                    "Skipping checkout/rebase (resuming at AI step)".to_string(),
                );
                return IterationState::RunningAi;
            }
        } else {
            self.ctx.iteration += 1;
        }

        IterationState::Checkout
    }

    async fn do_checkout(&mut self) -> IterationState {
        self.emit_status(SessionStatus::Running {
            step: SessionStep::Checkout,
            iteration: self.ctx.iteration,
        });
        self.emit_event(SessionEventPayload::Housekeeping {
            block: HousekeepingBlock::StepStarted {
                step: SessionStep::Checkout,
                description: "Checking out branch".to_string(),
            },
        });

        if let Err(e) = self.git.checkout_branch().await {
            self.try_git_recovery(&format!("{}", e), "Checkout").await;
            if let Err(e2) = self.git.checkout_branch().await {
                self.try_git_recovery_ai(&format!("{}", e2), "Checkout")
                    .await;
                if let Err(e3) = self.git.checkout_branch().await {
                    self.emit_log(
                        LogCategory::Error,
                        format!("Checkout failed after all recovery: {}", e3),
                    );
                    return IterationState::NewIteration;
                }
            }
        }

        IterationState::RebasePreAi
    }

    async fn do_rebase_pre_ai(&mut self) -> IterationState {
        self.emit_status(SessionStatus::Running {
            step: SessionStep::RebasePreAi,
            iteration: self.ctx.iteration,
        });
        self.emit_event(SessionEventPayload::Housekeeping {
            block: HousekeepingBlock::StepStarted {
                step: SessionStep::RebasePreAi,
                description: "Fetching and rebasing onto main".to_string(),
            },
        });

        self.fetch_main_with_recovery().await;

        match self.rebase_or_delegate_to_ai().await {
            Ok(()) => {
                if self.ctx.stash_pending {
                    IterationState::StashPop
                } else {
                    IterationState::RunningAi
                }
            }
            Err(e) => {
                self.emit_log(LogCategory::Error, format!("Pre-AI rebase failed: {}", e));
                self.ctx.recovery_error = Some(e);
                IterationState::WaitingForRecovery
            }
        }
    }

    async fn do_stash_pop(&mut self) -> IterationState {
        self.ctx.stash_pending = false;
        self.emit_log(LogCategory::Script, "Unstashing changes...".to_string());
        match self.git.run_in_worktree(&["stash", "pop"]).await {
            Ok(output) => self.emit_log(LogCategory::Git, output),
            Err(e) => {
                self.emit_log(
                    LogCategory::Warning,
                    format!("Stash pop failed: {}. Changes remain in stash.", e),
                );
            }
        }
        IterationState::RunningAi
    }

    async fn do_running_ai(&mut self) -> IterationState {
        self.ctx.ai_attempts += 1;

        if self.ctx.ai_attempts > MAX_AI_ATTEMPTS {
            return self.check_ai_result().await;
        }

        if self.ctx.ai_attempts == 1 {
            self.emit_status(SessionStatus::Running {
                step: SessionStep::RunningAi,
                iteration: self.ctx.iteration,
            });
            self.emit_log(
                LogCategory::Script,
                format!(
                    "Running {} (iteration {})...",
                    self.provider.name(),
                    self.ctx.iteration
                ),
            );

            match self.git.get_head().await {
                Ok(h) => self.ctx.head_before_ai = Some(h),
                Err(e) => {
                    self.emit_log(LogCategory::Error, format!("Failed to get HEAD: {}", e));
                    return IterationState::NewIteration;
                }
            }
            self.ctx.ai_ok = false;
        } else {
            self.emit_log(
                LogCategory::Script,
                format!(
                    "Resuming {} session (attempt {}/{})...",
                    self.provider.name(),
                    self.ctx.ai_attempts,
                    MAX_AI_ATTEMPTS
                ),
            );
        }

        let resume_id = self.ctx.ai_session_id.clone();
        let prompt = if resume_id.is_some() {
            "You are resuming a previous session that was interrupted. \
            Please continue where you left off. Check git status and your \
            previous work before starting new changes."
                .to_string()
        } else {
            match load_prompt(&self.config.prompt_file, &self.config.preamble) {
                Ok(p) => p,
                Err(e) => {
                    self.emit_status(SessionStatus::Failed {
                        error: format!("Failed to load prompt: {}", e),
                    });
                    return IterationState::Failed;
                }
            }
        };

        if let Some(ref rid) = resume_id {
            self.emit_log(
                LogCategory::Prompt,
                format!("Resuming session: {}\n{}", rid, prompt),
            );
        } else {
            self.emit_log(LogCategory::Prompt, prompt.clone());
        }

        let (output_tx, mut output_rx) = mpsc::unbounded_channel();
        let abort_clone = self.abort_rx.clone();
        let working_dir = self.git.worktree_dir();
        let provider_clone = self.provider.clone();
        let model_clone = self.config.model.clone();

        self.emit_agent_args_log(
            "AI",
            model_clone.as_deref(),
            resume_id.as_deref(),
            &working_dir,
        );

        let ai_task = tokio::spawn(async move {
            provider_clone
                .run(
                    &working_dir,
                    &prompt,
                    model_clone.as_deref(),
                    resume_id.as_deref(),
                    output_tx,
                    abort_clone,
                )
                .await
        });

        let mut rate_limited = false;
        while let Some(output) = output_rx.recv().await {
            match output {
                AiOutput::Text(text) => {
                    self.emit_event(SessionEventPayload::AiContent {
                        block: AiContentBlock::Text { text },
                    });
                }
                AiOutput::ToolUse { tool_id, tool } => {
                    self.emit_event(SessionEventPayload::AiContent {
                        block: AiContentBlock::ToolUse { tool_id, tool },
                    });
                }
                AiOutput::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    self.emit_event(SessionEventPayload::AiContent {
                        block: AiContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        },
                    });
                }
                AiOutput::RateLimited { message } => {
                    self.emit_event(SessionEventPayload::RateLimited {
                        message: message.clone(),
                    });
                    rate_limited = true;
                }
                AiOutput::Finished {
                    duration_secs,
                    cost_usd,
                } => {
                    let cost_str = cost_usd
                        .map(|c| format!(" | cost: ${:.4}", c))
                        .unwrap_or_default();
                    self.emit_log(
                        LogCategory::Script,
                        format!(
                            "--- {} finished in {:.1}s{} ---",
                            self.config.ai_tool, duration_secs, cost_str
                        ),
                    );
                }
                AiOutput::Error(e) => {
                    self.emit_log(LogCategory::Error, e);
                }
                AiOutput::SessionId(sid) => {
                    if self.ctx.ai_session_id.as_deref() != Some(&sid) {
                        self.ctx.ai_session_id = Some(sid.clone());
                        self.emit_event(SessionEventPayload::AiSessionIdChanged {
                            ai_session_id: Some(sid),
                        });
                    }
                }
            }
        }

        if rate_limited {
            ai_task.await.ok();
            self.emit_status(SessionStatus::Running {
                step: SessionStep::Paused,
                iteration: self.ctx.iteration,
            });
            return IterationState::RateLimitPause;
        }

        match ai_task.await {
            Ok(Ok(())) => {
                self.ctx.ai_ok = true;
            }
            Ok(Err(e)) => {
                self.emit_log(
                    LogCategory::Warning,
                    format!("{} failed: {}", self.config.ai_tool, e),
                );
                if self.ctx.ai_session_id.is_some() && self.ctx.ai_attempts < MAX_AI_ATTEMPTS {
                    return IterationState::AiRetry;
                }
            }
            Err(e) => {
                self.emit_log(
                    LogCategory::Warning,
                    format!("{} task panicked: {}", self.config.ai_tool, e),
                );
            }
        }

        self.check_ai_result().await
    }

    async fn do_rate_limit_pause(&mut self) -> IterationState {
        let pause_interval = std::time::Duration::from_secs(60);
        loop {
            if *self.abort_rx.borrow() || *self.stop_rx.borrow() {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep(pause_interval) => {}
                _ = self.abort_rx.changed() => { continue; }
                _ = self.stop_rx.changed() => { continue; }
            }
            self.emit_log(
                LogCategory::Script,
                "Rate limit may have reset, retrying...".to_string(),
            );
            break;
        }
        self.ctx.iteration -= 1;
        IterationState::RunningAi
    }

    async fn do_push_branch(&mut self) -> IterationState {
        self.emit_status(SessionStatus::Running {
            step: SessionStep::PushBranch,
            iteration: self.ctx.iteration,
        });
        self.emit_event(SessionEventPayload::Housekeeping {
            block: HousekeepingBlock::StepStarted {
                step: SessionStep::PushBranch,
                description: "Pushing branch to origin".to_string(),
            },
        });

        if let Err(e) = self.git.push_branch().await {
            self.try_git_recovery(&format!("{}", e), "Push branch")
                .await;
            if let Err(e2) = self.git.push_branch().await {
                self.try_git_recovery_ai(&format!("{}", e2), "Push branch")
                    .await;
                if let Err(e3) = self.git.push_branch().await {
                    self.emit_log(
                        LogCategory::Warning,
                        format!("Git housekeeping failed: {}. Continuing...", e3),
                    );
                    return IterationState::NewIteration;
                }
            }
        }

        self.emit_event(SessionEventPayload::Housekeeping {
            block: HousekeepingBlock::StepCompleted {
                step: SessionStep::PushBranch,
                summary: "Branch pushed".to_string(),
            },
        });

        IterationState::RebasePostAi
    }

    async fn do_rebase_post_ai(&mut self) -> IterationState {
        self.emit_status(SessionStatus::Running {
            step: SessionStep::RebasePostAi,
            iteration: self.ctx.iteration,
        });
        self.emit_event(SessionEventPayload::Housekeeping {
            block: HousekeepingBlock::StepStarted {
                step: SessionStep::RebasePostAi,
                description: "Rebasing onto main".to_string(),
            },
        });

        self.fetch_main_with_recovery().await;

        if let Err(e) = self.rebase_or_delegate_to_ai().await {
            self.emit_log(LogCategory::Error, format!("Post-AI rebase failed: {}", e));
            self.ctx.recovery_error = Some(e);
            return IterationState::WaitingForRecovery;
        }

        self.emit_event(SessionEventPayload::Housekeeping {
            block: HousekeepingBlock::StepCompleted {
                step: SessionStep::RebasePostAi,
                summary: "Rebased onto main".to_string(),
            },
        });

        if let Ok(diff_stat) = self.git.diff_stat_against_main().await {
            if !diff_stat.trim().is_empty() {
                self.emit_event(SessionEventPayload::Housekeeping {
                    block: HousekeepingBlock::DiffStat { stat: diff_stat },
                });
            }
        }

        IterationState::PushToMain
    }

    async fn do_push_to_main(&mut self) -> IterationState {
        self.emit_status(SessionStatus::Running {
            step: SessionStep::PushToMain,
            iteration: self.ctx.iteration,
        });
        self.emit_event(SessionEventPayload::Housekeeping {
            block: HousekeepingBlock::StepStarted {
                step: SessionStep::PushToMain,
                description: "Pushing to main".to_string(),
            },
        });

        if let Err(e) = self.git.push_to_main().await {
            self.try_git_recovery(&format!("{}", e), "Push to main")
                .await;
            if let Err(e2) = self.git.push_to_main().await {
                self.try_git_recovery_ai(&format!("{}", e2), "Push to main")
                    .await;
                if let Err(e3) = self.git.push_to_main().await {
                    self.emit_log(
                        LogCategory::Warning,
                        format!("Git housekeeping failed: {}. Continuing...", e3),
                    );
                    return IterationState::NewIteration;
                }
            }
        }

        self.emit_event(SessionEventPayload::Housekeeping {
            block: HousekeepingBlock::StepCompleted {
                step: SessionStep::PushToMain,
                summary: "Pushed to main".to_string(),
            },
        });

        IterationState::Tagging
    }

    async fn do_tagging(&mut self) -> IterationState {
        if !self.config.tagging_enabled {
            return IterationState::IterationComplete;
        }

        self.emit_status(SessionStatus::Running {
            step: SessionStep::Tagging,
            iteration: self.ctx.iteration,
        });
        self.emit_event(SessionEventPayload::Housekeeping {
            block: HousekeepingBlock::StepStarted {
                step: SessionStep::Tagging,
                description: "Creating and pushing tag".to_string(),
            },
        });

        match self.git.tag_and_push().await {
            Ok(new_tag) => {
                self.emit_event(SessionEventPayload::Housekeeping {
                    block: HousekeepingBlock::StepCompleted {
                        step: SessionStep::Tagging,
                        summary: format!("Tagged {}", new_tag),
                    },
                });
                self.ctx.last_tag = Some(new_tag);
            }
            Err(e) => {
                self.emit_log(LogCategory::Warning, format!("Tagging failed: {}", e));
            }
        }

        IterationState::IterationComplete
    }

    fn do_iteration_complete(&mut self) -> IterationState {
        self.ctx.ai_session_id = None;
        self.emit_event(SessionEventPayload::AiSessionIdChanged {
            ai_session_id: None,
        });
        self.emit_event(SessionEventPayload::IterationComplete {
            iteration: self.ctx.iteration,
            tag: self.ctx.last_tag.clone(),
        });

        let timestamp = simple_timestamp();
        if let Some(ref t) = self.ctx.last_tag {
            self.emit_log(
                LogCategory::Script,
                format!(
                    "=== Iteration {} complete: tagged {} ({}) ===",
                    self.ctx.iteration, t, timestamp
                ),
            );
        } else {
            self.emit_log(
                LogCategory::Script,
                format!(
                    "=== Iteration {} complete ({}) ===",
                    self.ctx.iteration, timestamp
                ),
            );
        }

        IterationState::NewIteration
    }

    async fn do_waiting_for_recovery(&mut self) -> IterationState {
        let error = self.ctx.recovery_error.take().unwrap_or_default();

        self.emit_event(SessionEventPayload::ActionRequired {
            error: error.clone(),
            options: vec![
                RecoveryOption {
                    id: "commit".to_string(),
                    label: "Commit changes".to_string(),
                    description:
                        "Launch the AI agent to commit the uncommitted changes, then retry"
                            .to_string(),
                },
                RecoveryOption {
                    id: "stash".to_string(),
                    label: "Stash changes".to_string(),
                    description: "Run 'git stash' to save uncommitted changes, then retry"
                        .to_string(),
                },
                RecoveryOption {
                    id: "reset".to_string(),
                    label: "Hard reset".to_string(),
                    description: "Run 'git reset --hard' to discard all uncommitted changes"
                        .to_string(),
                },
                RecoveryOption {
                    id: "abort".to_string(),
                    label: "Stop session".to_string(),
                    description: "Stop the session without changing anything".to_string(),
                },
            ],
        });

        match self.action_rx.recv().await {
            Some(RecoveryAction::Stash) => {
                self.emit_log(LogCategory::Script, "Stashing changes...".to_string());
                match self.git.run_in_worktree(&["stash"]).await {
                    Ok(output) => self.emit_log(LogCategory::Git, output),
                    Err(e) => {
                        self.emit_log(LogCategory::Error, format!("Stash failed: {}", e));
                        self.emit_status(SessionStatus::Failed {
                            error: format!("Stash failed: {}", e),
                        });
                        self.emit_event(SessionEventPayload::Finished {
                            reason: "Stash failed".to_string(),
                        });
                        return IterationState::Failed;
                    }
                }
                self.ctx.stash_pending = true;
                self.emit_log(
                    LogCategory::Script,
                    "Retrying (will unstash after rebase)...".to_string(),
                );
                IterationState::NewIteration
            }
            Some(RecoveryAction::Commit) => {
                self.emit_log(
                    LogCategory::Script,
                    "Invoking AI agent to commit changes...".to_string(),
                );
                self.run_commit_agent().await
            }
            Some(RecoveryAction::HardReset) => {
                self.emit_log(LogCategory::Script, "Resetting working tree...".to_string());
                match self.git.run_in_worktree(&["reset", "--hard"]).await {
                    Ok(output) => self.emit_log(LogCategory::Git, output),
                    Err(e) => {
                        self.emit_log(LogCategory::Error, format!("Reset failed: {}", e));
                        self.emit_status(SessionStatus::Failed {
                            error: format!("Reset failed: {}", e),
                        });
                        self.emit_event(SessionEventPayload::Finished {
                            reason: "Reset failed".to_string(),
                        });
                        return IterationState::Failed;
                    }
                }
                self.emit_log(LogCategory::Script, "Retrying...".to_string());
                IterationState::NewIteration
            }
            Some(RecoveryAction::Abort) | None => {
                self.emit_status(SessionStatus::Failed { error });
                self.emit_event(SessionEventPayload::Finished {
                    reason: "Stopped by user".to_string(),
                });
                IterationState::Failed
            }
        }
    }

    // ── Post-AI check ───────────────────────────────────────────────────

    async fn check_ai_result(&mut self) -> IterationState {
        if *self.abort_rx.borrow() {
            return IterationState::Done;
        }

        if !self.ctx.ai_ok {
            self.emit_log(
                LogCategory::Warning,
                format!("{} exited with an error.", self.config.ai_tool),
            );
        }

        let head_before = self
            .ctx
            .head_before_ai
            .as_ref()
            .expect("head_before_ai should be set");
        let head_changed = self.git.head_changed(head_before).await.unwrap_or(false);

        if !head_changed {
            self.emit_log(
                LogCategory::Warning,
                format!(
                    "{} made no commits. Skipping housekeeping.",
                    self.config.ai_tool
                ),
            );
            self.ctx.ai_session_id = None;
            self.emit_event(SessionEventPayload::AiSessionIdChanged {
                ai_session_id: None,
            });
            return IterationState::NewIteration;
        }

        if *self.stop_rx.borrow() {
            self.emit_log(
                LogCategory::Script,
                "Stop requested — pushing commits before exiting...".to_string(),
            );
        }

        IterationState::PushBranch
    }

    // ── Commit recovery agent ───────────────────────────────────────────

    async fn run_commit_agent(&mut self) -> IterationState {
        let commit_prompt = "There are uncommitted changes in this git repository. \
            Please review the changes with 'git diff' and 'git status', then stage and commit them \
            with an appropriate commit message describing what was changed. \
            Do not amend existing commits. Create a new commit.";

        let (output_tx, mut output_rx) = mpsc::unbounded_channel();
        let abort_clone = self.abort_rx.clone();
        let working_dir = self.git.worktree_dir();
        let provider_clone = self.provider.clone();

        self.emit_agent_args_log("Commit agent", None, None, &working_dir);

        let commit_task = tokio::spawn(async move {
            provider_clone
                .run(
                    &working_dir,
                    commit_prompt,
                    None,
                    None,
                    output_tx,
                    abort_clone,
                )
                .await
        });

        while let Some(output) = output_rx.recv().await {
            match output {
                AiOutput::Text(text) => {
                    self.emit_event(SessionEventPayload::AiContent {
                        block: AiContentBlock::Text { text },
                    });
                }
                AiOutput::ToolUse { tool_id, tool } => {
                    self.emit_event(SessionEventPayload::AiContent {
                        block: AiContentBlock::ToolUse { tool_id, tool },
                    });
                }
                AiOutput::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    self.emit_event(SessionEventPayload::AiContent {
                        block: AiContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        },
                    });
                }
                AiOutput::Finished {
                    duration_secs,
                    cost_usd,
                } => {
                    let cost_str = cost_usd
                        .map(|c| format!(" | cost: ${:.4}", c))
                        .unwrap_or_default();
                    self.emit_log(
                        LogCategory::Script,
                        format!(
                            "--- Commit agent finished in {:.1}s{} ---",
                            duration_secs, cost_str
                        ),
                    );
                }
                AiOutput::Error(e) => self.emit_log(LogCategory::Error, e),
                AiOutput::RateLimited { message } => {
                    self.emit_event(SessionEventPayload::RateLimited { message });
                }
                AiOutput::SessionId(_) => {}
            }
        }

        match commit_task.await {
            Ok(Ok(())) => {
                self.emit_log(
                    LogCategory::Script,
                    "Commit complete. Retrying...".to_string(),
                );
                IterationState::NewIteration
            }
            Ok(Err(e)) => {
                self.emit_log(LogCategory::Error, format!("Commit agent failed: {}", e));
                self.emit_status(SessionStatus::Failed {
                    error: format!("Commit agent failed: {}", e),
                });
                self.emit_event(SessionEventPayload::Finished {
                    reason: "Commit failed".to_string(),
                });
                IterationState::Failed
            }
            Err(e) => {
                self.emit_log(LogCategory::Error, format!("Commit agent panicked: {}", e));
                self.emit_status(SessionStatus::Failed {
                    error: format!("Commit agent panicked: {}", e),
                });
                self.emit_event(SessionEventPayload::Finished {
                    reason: "Commit failed".to_string(),
                });
                IterationState::Failed
            }
        }
    }

    // ── Emit helpers ────────────────────────────────────────────────────

    fn emit_event(&self, payload: SessionEventPayload) {
        (self.emit)(SessionEvent {
            session_id: self.session_id.clone(),
            iteration: self.event_iteration(),
            payload,
        });
    }

    /// The iteration to stamp on outgoing events. Setup logs run before the
    /// first `do_new_iteration` increment, so anchor them to iteration 1 to
    /// keep persisted log files aligned with what users see.
    fn event_iteration(&self) -> u32 {
        self.ctx.iteration.max(1)
    }

    fn emit_log(&self, category: LogCategory, text: String) {
        self.emit_event(SessionEventPayload::Log { category, text });
    }

    fn emit_status(&self, status: SessionStatus) {
        self.emit_event(SessionEventPayload::StatusChanged { status });
    }

    /// Log the non-prompt arguments we hand to an AI agent invocation —
    /// the tool, the model, the resume id, and the working directory. The
    /// prompt itself is omitted (it's already emitted as a Prompt log line
    /// where it matters, and inlining it here would dwarf the other args).
    fn emit_agent_args_log(
        &self,
        label: &str,
        model: Option<&str>,
        resume_id: Option<&str>,
        working_dir: &std::path::Path,
    ) {
        self.emit_log(
            LogCategory::Script,
            format!(
                "{} args: tool={} model={} resume={} cwd={}",
                label,
                self.config.ai_tool,
                model.unwrap_or("<default>"),
                resume_id.unwrap_or("<none>"),
                working_dir.display(),
            ),
        );
    }

    // ── Fetch main with recovery ────────────────────────────────────────

    async fn fetch_main_with_recovery(&mut self) {
        if let Err(e) = self.git.fetch_main().await {
            self.emit_log(
                LogCategory::Warning,
                format!("Fetch failed: {} — attempting recovery", e),
            );
            self.try_git_recovery(&format!("{}", e), "Fetch main").await;
            if let Err(e2) = self.git.fetch_main().await {
                self.try_git_recovery_ai(&format!("{}", e2), "Fetch main")
                    .await;
                if let Err(e3) = self.git.fetch_main().await {
                    self.emit_log(
                        LogCategory::Warning,
                        format!("Fetch still failing: {}. Proceeding with stale main.", e3),
                    );
                }
            }
        }
    }

    // ── Git recovery helpers ────────────────────────────────────────────

    /// Try to rebase onto origin/main. On any failure, abort the rebase, hand
    /// the task to an AI agent, and verify the result. The agent gets the
    /// full task ("Rebase onto origin/<base>") rather than a partial state to
    /// patch up, so it can choose how to resolve conflicts and commit them.
    /// If the agent can't produce a clean rebase, surface the failure to the
    /// caller — `do_rebase_pre_ai` / `do_rebase_post_ai` will route it to the
    /// user via the multi-choice recovery prompt.
    async fn rebase_or_delegate_to_ai(&mut self) -> Result<(), String> {
        let initial_err = match self.git.rebase_onto_main().await {
            Ok(output) => {
                if !output.trim().is_empty() {
                    self.emit_log(LogCategory::Git, output);
                }
                return Ok(());
            }
            Err(RebaseError::Conflict(e)) | Err(RebaseError::Permanent(e)) => e,
        };

        self.emit_log(
            LogCategory::Warning,
            format!(
                "Rebase failed: {} — aborting and delegating to AI agent",
                initial_err
            ),
        );

        // Always leave a clean slate before the agent runs so it doesn't
        // inherit a half-applied rebase from our attempt.
        self.git.abort_rebase().await.ok();

        let main_branch = self.config.main_branch.clone();
        self.emit_log(
            LogCategory::Script,
            format!("Running agentic rebase onto origin/{}...", main_branch),
        );
        let prompt = format!(
            "Rebase the current branch onto origin/{branch}.\n\n\
             Run `git rebase origin/{branch}` from the current working directory. \
             If conflicts arise, resolve them, stage the resolved files, and \
             continue with `git rebase --continue`. When you are done, the \
             branch must be cleanly rebased on top of origin/{branch}, with \
             no active rebase state and a clean working tree. If you cannot \
             complete the rebase, run `git rebase --abort` and exit with a \
             non-zero status.",
            branch = main_branch
        );

        let agent_ok = self.run_one_shot_agent(prompt, "Rebase agent").await;

        let on_target = self.git.verify_main_is_ancestor().await.unwrap_or(false);
        let no_active_rebase = !self.git.has_active_rebase();

        if agent_ok && on_target && no_active_rebase {
            return Ok(());
        }

        // Don't leave a half-applied rebase behind for the recovery prompt.
        if self.git.has_active_rebase() {
            self.git.abort_rebase().await.ok();
        }

        let reason = if !agent_ok {
            "Rebase agent exited with an error".to_string()
        } else if self.git.has_active_rebase() {
            "Rebase agent left an in-progress rebase".to_string()
        } else if !on_target {
            format!(
                "Branch is not rebased onto origin/{} after agent run",
                main_branch
            )
        } else {
            "Rebase did not complete cleanly".to_string()
        };
        Err(reason)
    }

    /// Run a one-shot AI agent with a custom prompt. Forwards the agent's
    /// streaming output (text, tool use/results, rate-limit signals) to the
    /// session log. Returns whether the agent process exited successfully.
    async fn run_one_shot_agent(&mut self, prompt: String, label: &str) -> bool {
        let (output_tx, mut output_rx) = mpsc::unbounded_channel();
        let abort_clone = self.abort_rx.clone();
        let working_dir = self.git.worktree_dir();
        let provider_clone = self.provider.clone();

        self.emit_agent_args_log(label, None, None, &working_dir);

        let task = tokio::spawn(async move {
            provider_clone
                .run(&working_dir, &prompt, None, None, output_tx, abort_clone)
                .await
        });

        while let Some(output) = output_rx.recv().await {
            match output {
                AiOutput::Text(text) => {
                    self.emit_event(SessionEventPayload::AiContent {
                        block: AiContentBlock::Text { text },
                    });
                }
                AiOutput::ToolUse { tool_id, tool } => {
                    self.emit_event(SessionEventPayload::AiContent {
                        block: AiContentBlock::ToolUse { tool_id, tool },
                    });
                }
                AiOutput::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    self.emit_event(SessionEventPayload::AiContent {
                        block: AiContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        },
                    });
                }
                AiOutput::Finished {
                    duration_secs,
                    cost_usd,
                } => {
                    let cost_str = cost_usd
                        .map(|c| format!(" | cost: ${:.4}", c))
                        .unwrap_or_default();
                    self.emit_log(
                        LogCategory::Script,
                        format!(
                            "--- {} finished in {:.1}s{} ---",
                            label, duration_secs, cost_str
                        ),
                    );
                }
                AiOutput::Error(e) => self.emit_log(LogCategory::Error, e),
                AiOutput::RateLimited { message } => {
                    self.emit_event(SessionEventPayload::RateLimited { message });
                }
                AiOutput::SessionId(_) => {}
            }
        }

        match task.await {
            Ok(Ok(())) => true,
            Ok(Err(e)) => {
                self.emit_log(LogCategory::Error, format!("{} failed: {}", label, e));
                false
            }
            Err(e) => {
                self.emit_log(LogCategory::Error, format!("{} panicked: {}", label, e));
                false
            }
        }
    }

    async fn heal_git_state(&mut self) {
        // Extract emit to avoid holding &self across the await
        let emit_fn = self.emit;
        let sid = self.session_id.clone();
        let iteration = self.event_iteration();
        let emit_log = move |cat: LogCategory, text: String| {
            (emit_fn)(SessionEvent {
                session_id: sid.clone(),
                iteration,
                payload: SessionEventPayload::Log {
                    category: cat,
                    text,
                },
            });
        };
        self.git.remove_stale_lock_files(&emit_log).await;
        if self.git.has_active_rebase() {
            self.emit_log(
                LogCategory::Warning,
                "Aborting leftover rebase...".to_string(),
            );
            self.git.abort_rebase().await.ok();
        }
        self.git.run_in_worktree(&["reset", "--hard"]).await.ok();
    }

    async fn recover_with_ai(&mut self, error_msg: &str) -> bool {
        let prompt = format!(
            "A git operation in this repository failed with the following error:\n\n\
             {}\n\n\
             The working directory is: {}\n\n\
             Diagnose the issue and fix it. Common problems include:\n\
             - Stale lock files (index.lock, HEAD.lock) — remove them\n\
             - Unmerged files — resolve conflicts, stage, and complete the operation\n\
             - Dirty worktree — commit or stash changes as appropriate\n\
             - Detached HEAD — checkout the correct branch\n\n\
             After fixing, make sure `git status` shows a clean state and the branch can be checked out.",
            error_msg,
            self.git.worktree_dir().display(),
        );

        let (output_tx, mut output_rx) = mpsc::unbounded_channel();
        let abort_clone = self.abort_rx.clone();
        let working_dir = self.git.worktree_dir();
        let provider_clone = self.provider.clone();

        self.emit_agent_args_log("Git recovery agent", None, None, &working_dir);

        let task = tokio::spawn(async move {
            provider_clone
                .run(&working_dir, &prompt, None, None, output_tx, abort_clone)
                .await
        });

        while let Some(output) = output_rx.recv().await {
            match output {
                AiOutput::Text(text) => {
                    self.emit_event(SessionEventPayload::AiContent {
                        block: AiContentBlock::Text { text },
                    });
                }
                AiOutput::ToolUse { tool_id, tool } => {
                    self.emit_event(SessionEventPayload::AiContent {
                        block: AiContentBlock::ToolUse { tool_id, tool },
                    });
                }
                AiOutput::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    self.emit_event(SessionEventPayload::AiContent {
                        block: AiContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        },
                    });
                }
                AiOutput::Finished {
                    duration_secs,
                    cost_usd,
                } => {
                    let cost_str = cost_usd
                        .map(|c| format!(" | cost: ${:.4}", c))
                        .unwrap_or_default();
                    self.emit_log(
                        LogCategory::Script,
                        format!(
                            "--- Git recovery agent finished in {:.1}s{} ---",
                            duration_secs, cost_str
                        ),
                    );
                }
                AiOutput::Error(e) => self.emit_log(LogCategory::Error, e),
                AiOutput::RateLimited { message } => {
                    self.emit_event(SessionEventPayload::RateLimited { message });
                }
                AiOutput::SessionId(_) => {}
            }
        }

        match task.await {
            Ok(Ok(())) => true,
            Ok(Err(e)) => {
                self.emit_log(
                    LogCategory::Error,
                    format!("Git recovery agent failed: {}", e),
                );
                false
            }
            Err(e) => {
                self.emit_log(
                    LogCategory::Error,
                    format!("Git recovery agent panicked: {}", e),
                );
                false
            }
        }
    }

    async fn try_git_recovery(&mut self, error_msg: &str, op_name: &str) -> bool {
        self.emit_log(
            LogCategory::Warning,
            format!("{} failed: {} — attempting self-heal", op_name, error_msg),
        );
        self.heal_git_state().await;
        true
    }

    async fn try_git_recovery_ai(&mut self, error_msg: &str, op_name: &str) -> bool {
        self.emit_log(
            LogCategory::Warning,
            format!(
                "{} still failing after self-heal: {} — invoking AI recovery agent",
                op_name, error_msg
            ),
        );
        self.recover_with_ai(error_msg).await
    }
}

fn simple_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02} UTC", hours, mins, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use crate::git::ops::{GitOperations, RebaseError};
    use crate::provider::{AiOutput, AiProvider};

    // ── Test doubles ────────────────────────────────────────────────────

    struct TestGitOps {
        dir: PathBuf,
        active_rebase: Mutex<bool>,
        head_sha: Mutex<String>,
        head_changed_val: Mutex<bool>,
        main_ancestor_val: Mutex<bool>,
        rebase_results: Mutex<Vec<Result<String, RebaseError>>>,
    }

    impl TestGitOps {
        fn new() -> Self {
            Self {
                dir: PathBuf::from("/tmp/test-worktree"),
                active_rebase: Mutex::new(false),
                head_sha: Mutex::new("abc123".to_string()),
                head_changed_val: Mutex::new(true),
                main_ancestor_val: Mutex::new(true),
                rebase_results: Mutex::new(Vec::new()),
            }
        }

        fn set_head_changed(&self, val: bool) {
            *self.head_changed_val.lock().unwrap() = val;
        }

        fn set_main_ancestor(&self, val: bool) {
            *self.main_ancestor_val.lock().unwrap() = val;
        }

        fn push_rebase_result(&self, result: Result<String, RebaseError>) {
            self.rebase_results.lock().unwrap().push(result);
        }
    }

    #[async_trait::async_trait]
    impl GitOperations for TestGitOps {
        fn worktree_dir(&self) -> PathBuf {
            self.dir.clone()
        }
        fn has_active_rebase(&self) -> bool {
            *self.active_rebase.lock().unwrap()
        }
        async fn ensure_branch_exists(&self) -> anyhow::Result<()> {
            Ok(())
        }
        async fn ensure_worktree(&self) -> anyhow::Result<()> {
            Ok(())
        }
        async fn checkout_branch(&self) -> anyhow::Result<()> {
            Ok(())
        }
        async fn fetch_main(&self) -> anyhow::Result<String> {
            Ok(String::new())
        }
        async fn rebase_onto_main(&self) -> Result<String, RebaseError> {
            let mut results = self.rebase_results.lock().unwrap();
            if results.is_empty() {
                Ok(String::new())
            } else {
                results.remove(0)
            }
        }
        async fn abort_rebase(&self) -> anyhow::Result<()> {
            Ok(())
        }
        async fn push_branch(&self) -> anyhow::Result<String> {
            Ok(String::new())
        }
        async fn push_to_main(&self) -> anyhow::Result<String> {
            Ok(String::new())
        }
        async fn get_head(&self) -> anyhow::Result<String> {
            Ok(self.head_sha.lock().unwrap().clone())
        }
        async fn head_changed(&self, _before: &str) -> anyhow::Result<bool> {
            Ok(*self.head_changed_val.lock().unwrap())
        }
        async fn tag_and_push(&self) -> anyhow::Result<String> {
            Ok("0.0.1".to_string())
        }
        async fn diff_stat_against_main(&self) -> anyhow::Result<String> {
            Ok(String::new())
        }
        async fn verify_main_is_ancestor(&self) -> anyhow::Result<bool> {
            Ok(*self.main_ancestor_val.lock().unwrap())
        }
        async fn run_in_worktree(&self, _args: &[&str]) -> Result<String, String> {
            Ok(String::new())
        }
        async fn remove_stale_lock_files(
            &self,
            _emit_log: &(dyn Fn(crate::events::LogCategory, String) + Send + Sync),
        ) {
        }
    }

    struct TestAiProvider;

    #[async_trait::async_trait]
    impl AiProvider for TestAiProvider {
        fn name(&self) -> &str {
            "test"
        }
        async fn run(
            &self,
            _working_dir: &std::path::Path,
            _prompt: &str,
            _model: Option<&str>,
            _resume_session_id: Option<&str>,
            output_tx: tokio::sync::mpsc::UnboundedSender<AiOutput>,
            _abort_rx: tokio::sync::watch::Receiver<bool>,
        ) -> anyhow::Result<()> {
            output_tx
                .send(AiOutput::Text("test output".to_string()))
                .ok();
            output_tx
                .send(AiOutput::Finished {
                    duration_secs: 1.0,
                    cost_usd: None,
                })
                .ok();
            Ok(())
        }
    }

    fn default_config() -> SessionConfig {
        SessionConfig {
            project_dir: PathBuf::from("/tmp/test"),
            mode: "test".to_string(),
            prompt_file: PathBuf::from("/tmp/test/PROMPT-test.md"),
            branch_name: "test-branch".to_string(),
            main_branch: "main".to_string(),
            preamble: String::new(),
            tagging_enabled: true,
            ai_tool: crate::provider::AiTool::Claude,
            model: None,
        }
    }

    fn default_context() -> SessionContext {
        SessionContext {
            iteration: 0,
            ai_session_id: None,
            stash_pending: false,
            head_before_ai: None,
            ai_attempts: 0,
            ai_ok: false,
            skip_to_step: None,
            recovery_error: None,
            last_tag: None,
        }
    }

    /// Helper: build a machine and run it, returning collected events.
    async fn run_machine_with(
        git: &dyn GitOperations,
        config: &SessionConfig,
        ctx: SessionContext,
        stop_before_start: bool,
    ) -> Vec<SessionEventPayload> {
        let provider: Arc<dyn AiProvider> = Arc::new(TestAiProvider);
        let events: Arc<Mutex<Vec<SessionEventPayload>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let emit: Box<dyn Fn(SessionEvent) + Send + Sync> = Box::new(move |event: SessionEvent| {
            events_clone.lock().unwrap().push(event.payload);
        });
        // Leak to get 'static lifetime (acceptable in tests).
        let emit_ref: &'static (dyn Fn(SessionEvent) + Send + Sync) = Box::leak(emit);

        let (stop_tx, stop_rx) = watch::channel(stop_before_start);
        let (_abort_tx, abort_rx) = watch::channel(false);
        let (_action_tx, action_rx) = mpsc::channel(1);

        let mut machine = SessionMachine::new(
            "test-session".to_string(),
            config,
            git,
            provider,
            emit_ref,
            stop_rx,
            abort_rx,
            action_rx,
            ctx,
        );

        // If not stopping immediately, signal stop after setup so we only run 1 iteration
        if !stop_before_start {
            let stop_tx_clone = stop_tx.clone();
            tokio::spawn(async move {
                // Give the machine time to pass through Setup and NewIteration
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                stop_tx_clone.send(true).ok();
            });
        }

        machine.run().await;

        let result = events.lock().unwrap().clone();
        result
    }

    // ── Tests ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn stop_before_start_exits_immediately() {
        let git = TestGitOps::new();
        let config = default_config();
        let ctx = default_context();

        let events = run_machine_with(&git, &config, ctx, true).await;

        // Should see Setup logs, then stop at NewIteration
        let has_finished = events
            .iter()
            .any(|e| matches!(e, SessionEventPayload::Finished { .. }));
        assert!(has_finished, "Machine should emit Finished when stopped");

        let has_stopped = events.iter().any(|e| {
            matches!(
                e,
                SessionEventPayload::StatusChanged {
                    status: SessionStatus::Stopped
                }
            )
        });
        assert!(has_stopped, "Machine should emit Stopped status");
    }

    #[tokio::test]
    async fn resume_ai_session_runs_through_full_cycle() {
        let git = TestGitOps::new();
        let config = default_config();
        let mut ctx = default_context();
        // Set resume so it skips prompt file loading
        ctx.ai_session_id = Some("resume-123".to_string());
        ctx.skip_to_step = Some(SessionStep::RunningAi);

        let events = run_machine_with(&git, &config, ctx, false).await;

        // Should have gone through RunningAi -> PushBranch -> RebasePostAi -> PushToMain -> Tagging -> IterationComplete
        let has_iteration_complete = events
            .iter()
            .any(|e| matches!(e, SessionEventPayload::IterationComplete { .. }));
        assert!(
            has_iteration_complete,
            "Should complete at least one iteration"
        );

        let has_tag = events.iter().any(|e| {
            matches!(
                e,
                SessionEventPayload::IterationComplete { tag: Some(_), .. }
            )
        });
        assert!(has_tag, "Should have tagged the iteration");
    }

    #[tokio::test]
    async fn no_commits_skips_housekeeping() {
        let git = TestGitOps::new();
        git.set_head_changed(false);
        let config = default_config();
        let mut ctx = default_context();
        ctx.ai_session_id = Some("resume-456".to_string());
        ctx.skip_to_step = Some(SessionStep::RunningAi);

        let events = run_machine_with(&git, &config, ctx, false).await;

        // Should NOT have PushBranch step since no commits
        let has_push_step = events.iter().any(|e| {
            matches!(
                e,
                SessionEventPayload::Housekeeping {
                    block: HousekeepingBlock::StepStarted {
                        step: SessionStep::PushBranch,
                        ..
                    }
                }
            )
        });
        assert!(!has_push_step, "Should skip push when AI made no commits");

        // Should have a warning about no commits
        let has_no_commit_warning = events.iter().any(|e| {
            if let SessionEventPayload::Log { text, .. } = e {
                text.contains("no commits")
            } else {
                false
            }
        });
        assert!(has_no_commit_warning, "Should warn about no commits");
    }

    #[tokio::test]
    async fn rebase_failure_warns_then_delegates_to_ai_agent() {
        // First rebase call (pre-AI) fails. The new flow aborts the rebase
        // and runs an AI agent. With main_ancestor still true (default), the
        // AI is treated as having succeeded and the iteration continues —
        // crucially, the user is NOT prompted via ActionRequired.
        let git = TestGitOps::new();
        git.push_rebase_result(Err(RebaseError::Conflict("merge conflict".to_string())));

        let config = default_config();
        let mut ctx = default_context();
        // Avoid loading a prompt file from disk in the iteration's AI step;
        // we only care about the rebase-step behavior here.
        ctx.ai_session_id = Some("resume-rebase-test".to_string());

        let events = run_machine_with(&git, &config, ctx, false).await;

        let has_delegation_warning = events.iter().any(|e| {
            matches!(e, SessionEventPayload::Log {
                category: LogCategory::Warning,
                text,
            } if text.contains("delegating to AI"))
        });
        assert!(
            has_delegation_warning,
            "Should warn that the rebase is being delegated to the AI agent"
        );

        // The recovered rebase must NOT prompt the user when the agent
        // succeeds — that's the whole point of the new flow.
        let has_action_required = events
            .iter()
            .any(|e| matches!(e, SessionEventPayload::ActionRequired { .. }));
        assert!(
            !has_action_required,
            "Successful agent recovery must not surface the multi-choice prompt"
        );
    }

    #[tokio::test]
    async fn rebase_failure_with_unsuccessful_ai_emits_action_required() {
        // Rebase fails AND the agent fails to leave the branch rebased.
        // Expected: machine routes to WaitingForRecovery, emitting an
        // ActionRequired event so the user can choose Stash / Commit /
        // HardReset / Abort.
        let git = TestGitOps::new();
        git.push_rebase_result(Err(RebaseError::Conflict("stubborn conflict".to_string())));
        git.set_main_ancestor(false);

        let config = default_config();
        let ctx = default_context();

        // Use a separate channel so we can drop the action_tx, causing
        // action_rx.recv() to return None and the machine to fail cleanly.
        let provider: Arc<dyn AiProvider> = Arc::new(TestAiProvider);
        let events: Arc<Mutex<Vec<SessionEventPayload>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let emit: Box<dyn Fn(SessionEvent) + Send + Sync> = Box::new(move |event: SessionEvent| {
            events_clone.lock().unwrap().push(event.payload);
        });
        let emit_ref: &'static (dyn Fn(SessionEvent) + Send + Sync) = Box::leak(emit);

        let (_stop_tx, stop_rx) = watch::channel(false);
        let (_abort_tx, abort_rx) = watch::channel(false);
        let (action_tx, action_rx) = mpsc::channel(1);
        // Drop action_tx — recv() will return None and WaitingForRecovery
        // treats that as Abort, ending the machine deterministically.
        drop(action_tx);

        let mut machine = SessionMachine::new(
            "test-session".to_string(),
            &config,
            &git,
            provider,
            emit_ref,
            stop_rx,
            abort_rx,
            action_rx,
            ctx,
        );
        machine.run().await;

        let events = events.lock().unwrap().clone();
        let has_action_required = events
            .iter()
            .any(|e| matches!(e, SessionEventPayload::ActionRequired { .. }));
        assert!(
            has_action_required,
            "Failed-rebase + failed-AI must surface a multi-choice prompt"
        );
    }

    #[tokio::test]
    async fn post_ai_rebase_failure_also_prompts_user() {
        // Pre-AI rebase succeeds (no result pushed → default Ok). AI commits.
        // Post-AI rebase fails AND the agent fails to recover. Verify the
        // user is prompted in the post-AI step too — previously this step
        // silently dropped the iteration on the floor.
        let git = TestGitOps::new();
        // Skip the first (pre-AI) rebase call by leaving the queue empty —
        // that returns Ok. Stage failure for the post-AI call.
        git.push_rebase_result(Ok(String::new()));
        git.push_rebase_result(Err(RebaseError::Conflict("post-ai conflict".to_string())));
        // Make the agent's run look like it didn't actually rebase.
        git.set_main_ancestor(false);

        let config = default_config();
        let mut ctx = default_context();
        // Skip the prompt-file load in the AI step; we want to reach
        // do_rebase_post_ai, not stub out the whole iteration.
        ctx.ai_session_id = Some("resume-post-ai-test".to_string());

        let provider: Arc<dyn AiProvider> = Arc::new(TestAiProvider);
        let events: Arc<Mutex<Vec<SessionEventPayload>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let emit: Box<dyn Fn(SessionEvent) + Send + Sync> = Box::new(move |event: SessionEvent| {
            events_clone.lock().unwrap().push(event.payload);
        });
        let emit_ref: &'static (dyn Fn(SessionEvent) + Send + Sync) = Box::leak(emit);

        let (_stop_tx, stop_rx) = watch::channel(false);
        let (_abort_tx, abort_rx) = watch::channel(false);
        let (action_tx, action_rx) = mpsc::channel(1);
        drop(action_tx);

        let mut machine = SessionMachine::new(
            "test-session".to_string(),
            &config,
            &git,
            provider,
            emit_ref,
            stop_rx,
            abort_rx,
            action_rx,
            ctx,
        );
        machine.run().await;

        let events = events.lock().unwrap().clone();
        let has_post_ai_step = events.iter().any(|e| {
            matches!(
                e,
                SessionEventPayload::Housekeeping {
                    block: HousekeepingBlock::StepStarted {
                        step: SessionStep::RebasePostAi,
                        ..
                    }
                }
            )
        });
        assert!(
            has_post_ai_step,
            "Post-AI rebase step should run before failing"
        );

        let has_action_required = events
            .iter()
            .any(|e| matches!(e, SessionEventPayload::ActionRequired { .. }));
        assert!(
            has_action_required,
            "Post-AI rebase failure must also surface the multi-choice prompt"
        );
    }

    #[tokio::test]
    async fn setup_emits_worktree_path() {
        let git = TestGitOps::new();
        let config = default_config();
        let ctx = default_context();

        let events = run_machine_with(&git, &config, ctx, true).await;

        let has_worktree_log = events.iter().any(|e| {
            if let SessionEventPayload::Log { text, .. } = e {
                text.contains("/tmp/test-worktree")
            } else {
                false
            }
        });
        assert!(has_worktree_log, "Setup should log the worktree path");
    }

    #[tokio::test]
    async fn tagging_disabled_skips_tag() {
        let git = TestGitOps::new();
        let mut config = default_config();
        config.tagging_enabled = false;
        let mut ctx = default_context();
        ctx.ai_session_id = Some("resume-789".to_string());
        ctx.skip_to_step = Some(SessionStep::RunningAi);

        let events = run_machine_with(&git, &config, ctx, false).await;

        let has_tag_step = events.iter().any(|e| {
            matches!(
                e,
                SessionEventPayload::Housekeeping {
                    block: HousekeepingBlock::StepStarted {
                        step: SessionStep::Tagging,
                        ..
                    }
                }
            )
        });
        assert!(!has_tag_step, "Should skip tagging when disabled");

        let has_iteration_complete = events
            .iter()
            .any(|e| matches!(e, SessionEventPayload::IterationComplete { tag: None, .. }));
        assert!(
            has_iteration_complete,
            "Should complete iteration without tag"
        );
    }

    /// Capture full `SessionEvent`s (not just payloads) so tests can assert
    /// on the iteration stamp.
    async fn run_machine_capturing_events(
        git: &dyn GitOperations,
        config: &SessionConfig,
        ctx: SessionContext,
        stop_before_start: bool,
    ) -> Vec<SessionEvent> {
        let provider: Arc<dyn AiProvider> = Arc::new(TestAiProvider);
        let events: Arc<Mutex<Vec<SessionEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        let emit: Box<dyn Fn(SessionEvent) + Send + Sync> = Box::new(move |event: SessionEvent| {
            events_clone.lock().unwrap().push(event);
        });
        let emit_ref: &'static (dyn Fn(SessionEvent) + Send + Sync) = Box::leak(emit);

        let (stop_tx, stop_rx) = watch::channel(stop_before_start);
        let (_abort_tx, abort_rx) = watch::channel(false);
        let (_action_tx, action_rx) = mpsc::channel(1);

        let mut machine = SessionMachine::new(
            "test-session".to_string(),
            config,
            git,
            provider,
            emit_ref,
            stop_rx,
            abort_rx,
            action_rx,
            ctx,
        );

        if !stop_before_start {
            let stop_tx_clone = stop_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                stop_tx_clone.send(true).ok();
            });
        }

        machine.run().await;
        let result = events.lock().unwrap().clone();
        result
    }

    /// Regression: every event the machine emits must be stamped with the
    /// machine's current iteration. This is the single source of truth that
    /// downstream consumers (log file routing, frontend buckets) rely on. If
    /// any future change forgets to stamp, this test catches it.
    #[tokio::test]
    async fn machine_stamps_iteration_on_every_event() {
        let git = TestGitOps::new();
        let config = default_config();
        let ctx = default_context();

        let events = run_machine_capturing_events(&git, &config, ctx, true).await;

        assert!(!events.is_empty(), "machine should emit at least one event");
        for event in &events {
            assert!(
                event.iteration >= 1,
                "every event must be stamped with iteration >= 1, got {}: {:?}",
                event.iteration,
                event.payload
            );
        }
    }

    /// Regression for the resume-after-restart bug: when the machine resumes
    /// at iteration N, every event it emits during that iteration carries
    /// `iteration = N` — not 1, and not whatever stale state a downstream
    /// consumer might be holding.
    #[tokio::test]
    async fn resumed_machine_stamps_resume_iteration() {
        let git = TestGitOps::new();
        let config = default_config();
        let mut ctx = default_context();
        ctx.iteration = 7;
        ctx.ai_session_id = Some("resume-iter-7".to_string());
        ctx.skip_to_step = Some(SessionStep::RunningAi);

        let events = run_machine_capturing_events(&git, &config, ctx, false).await;

        // Every event up to and including IterationComplete{ iteration: 7 }
        // must be stamped 7. Events emitted after IterationComplete (i.e.,
        // for the next iteration's setup) get stamped 8.
        let mut saw_complete = false;
        for event in &events {
            if !saw_complete {
                assert_eq!(
                    event.iteration, 7,
                    "pre-IterationComplete event stamped with wrong iteration: {:?}",
                    event.payload
                );
            }
            if matches!(
                event.payload,
                SessionEventPayload::IterationComplete { iteration: 7, .. }
            ) {
                saw_complete = true;
            }
        }
        assert!(
            saw_complete,
            "machine should have completed iteration 7 in this scenario"
        );
    }
}
