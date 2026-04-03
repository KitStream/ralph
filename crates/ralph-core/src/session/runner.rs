use std::sync::Arc;

use tokio::sync::{mpsc, watch};

use crate::discovery::load_prompt;
use crate::events::{LogCategory, RecoveryAction, RecoveryOption, SessionEvent, SessionEventPayload};
use crate::git::ops::{GitOps, RebaseError};
use crate::provider::{create_provider, AiOutput, AiProvider};
use crate::session::state::{SessionConfig, SessionId, SessionStatus, SessionStep};

/// Distinguishes transient errors (retry next iteration) from permanent ones (stop session).
#[derive(Debug)]
enum RunError {
    Transient(String),
    Permanent(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Transient(e) => write!(f, "{}", e),
            RunError::Permanent(e) => write!(f, "{}", e),
        }
    }
}

/// Run the session iteration loop.
/// This is the core loop that both CLI and GUI call.
/// `emit` is called for each event (status changes, log lines, etc.).
pub async fn run_session(
    id: SessionId,
    config: SessionConfig,
    emit: impl Fn(SessionEvent) + Send + Sync + 'static,
    stop_rx: watch::Receiver<bool>,
    abort_rx: watch::Receiver<bool>,
    mut action_rx: mpsc::Receiver<RecoveryAction>,
    resume_ai_session_id: Option<String>,
) {
    let emit_event = |payload: SessionEventPayload| {
        emit(SessionEvent {
            session_id: id.to_string(),
            payload,
        });
    };

    let emit_log = |category: LogCategory, text: String| {
        emit_event(SessionEventPayload::Log { category, text });
    };

    let emit_status = |status: SessionStatus| {
        emit_event(SessionEventPayload::StatusChanged { status });
    };

    let git = GitOps::new(&config.project_dir, &config.branch_name, &config.main_branch);
    let provider: Arc<dyn AiProvider> = Arc::from(create_provider(&config.ai_tool));

    // Setup
    emit_log(LogCategory::Script, "Setting up branch and worktree...".to_string());

    if let Err(e) = git.ensure_branch_exists().await {
        emit_status(SessionStatus::Failed {
            error: format!("Failed to ensure branch: {}", e),
        });
        return;
    }

    if let Err(e) = git.ensure_worktree().await {
        emit_status(SessionStatus::Failed {
            error: format!("Failed to ensure worktree: {}", e),
        });
        return;
    }

    emit_log(
        LogCategory::Script,
        format!(
            "Running in worktree: {:?} (mode: {}, branch: {})",
            git.worktree_dir, config.mode, config.branch_name
        ),
    );

    let mut iteration = 0u32;
    // AI session ID for crash recovery resume. Set from Aborted state or captured during run.
    let mut current_ai_session_id: Option<String> = resume_ai_session_id;

    let mut stash_pending = false; // true if we stashed changes that need to be popped after rebase

    loop {
        if *stop_rx.borrow() {
            emit_log(LogCategory::Script, "Stop requested. Exiting.".to_string());
            break;
        }

        iteration += 1;

        // Step 0: Checkout branch (with full recovery pipeline)
        emit_status(SessionStatus::Running {
            step: SessionStep::Checkout,
            iteration,
        });
        if let Err(e) = git.checkout_branch().await {
            try_git_recovery(&git, &provider, &format!("{}", e), "Checkout", &emit_log, &abort_rx).await;
            if let Err(e2) = git.checkout_branch().await {
                try_git_recovery_ai(&git, &provider, &format!("{}", e2), "Checkout", &emit_log, &abort_rx).await;
                if let Err(e3) = git.checkout_branch().await {
                    emit_log(LogCategory::Error, format!("Checkout failed after all recovery: {}", e3));
                    continue;
                }
            }
        }

        // Step a: Rebase onto main
        emit_status(SessionStatus::Running {
            step: SessionStep::RebasePreAi,
            iteration,
        });
        emit_log(LogCategory::Git, "Fetching and rebasing onto main...".to_string());

        if let Err(e) = git.fetch_main().await {
            emit_log(LogCategory::Warning, format!("Fetch failed: {} — attempting recovery", e));
            try_git_recovery(&git, &provider, &format!("{}", e), "Fetch main", &emit_log, &abort_rx).await;
            if let Err(e2) = git.fetch_main().await {
                try_git_recovery_ai(&git, &provider, &format!("{}", e2), "Fetch main", &emit_log, &abort_rx).await;
                if let Err(e3) = git.fetch_main().await {
                    emit_log(LogCategory::Warning, format!("Fetch still failing: {}. Proceeding with stale main.", e3));
                }
            }
        }

        match rebase_with_conflict_resolution(&git, provider.clone(), &emit_log, &abort_rx).await {
            Ok(_) => {}
            Err(RunError::Permanent(e)) => {
                emit_log(LogCategory::Error, format!("Pre-AI rebase failed: {}", e));

                // Ask the user what to do
                emit_event(SessionEventPayload::ActionRequired {
                    error: e.clone(),
                    options: vec![
                        RecoveryOption {
                            id: "commit".to_string(),
                            label: "Commit changes".to_string(),
                            description: "Launch the AI agent to commit the uncommitted changes, then retry".to_string(),
                        },
                        RecoveryOption {
                            id: "stash".to_string(),
                            label: "Stash changes".to_string(),
                            description: "Run 'git stash' to save uncommitted changes, then retry".to_string(),
                        },
                        RecoveryOption {
                            id: "reset".to_string(),
                            label: "Hard reset".to_string(),
                            description: "Run 'git reset --hard' to discard all uncommitted changes".to_string(),
                        },
                        RecoveryOption {
                            id: "abort".to_string(),
                            label: "Stop session".to_string(),
                            description: "Stop the session without changing anything".to_string(),
                        },
                    ],
                });

                // Wait for user response
                match action_rx.recv().await {
                    Some(RecoveryAction::Stash) => {
                        emit_log(LogCategory::Script, "Stashing changes...".to_string());
                        match git.run_in_worktree(&["stash"]).await {
                            Ok(output) => emit_log(LogCategory::Git, output),
                            Err(e) => {
                                emit_log(LogCategory::Error, format!("Stash failed: {}", e));
                                emit_status(SessionStatus::Failed { error: format!("Stash failed: {}", e) });
                                emit_event(SessionEventPayload::Finished { reason: "Stash failed".to_string() });
                                return;
                            }
                        }
                        stash_pending = true;
                        emit_log(LogCategory::Script, "Retrying (will unstash after rebase)...".to_string());
                        continue;
                    }
                    Some(RecoveryAction::Commit) => {
                        emit_log(LogCategory::Script, "Invoking AI agent to commit changes...".to_string());

                        let commit_prompt = "There are uncommitted changes in this git repository. \
                            Please review the changes with 'git diff' and 'git status', then stage and commit them \
                            with an appropriate commit message describing what was changed. \
                            Do not amend existing commits. Create a new commit.";

                        let (output_tx, mut output_rx) = mpsc::unbounded_channel();
                        let abort_clone = abort_rx.clone();
                        let working_dir = git.worktree_dir.clone();
                        let provider_clone = provider.clone();

                        let commit_task = tokio::spawn(async move {
                            provider_clone
                                .run(&working_dir, commit_prompt, None, output_tx, abort_clone)
                                .await
                        });

                        while let Some(output) = output_rx.recv().await {
                            match output {
                                AiOutput::Text(text) => emit_log(LogCategory::Ai, text),
                                AiOutput::Finished { duration_secs, cost_usd } => {
                                    let cost_str = cost_usd.map(|c| format!(" | cost: ${:.4}", c)).unwrap_or_default();
                                    emit_log(LogCategory::Script, format!("--- Commit agent finished in {:.1}s{} ---", duration_secs, cost_str));
                                }
                                AiOutput::Error(e) => emit_log(LogCategory::Error, e),
                                AiOutput::SessionId(_) => {} // not used for recovery
                            }
                        }

                        match commit_task.await {
                            Ok(Ok(())) => {
                                emit_log(LogCategory::Script, "Commit complete. Retrying...".to_string());
                            }
                            Ok(Err(e)) => {
                                emit_log(LogCategory::Error, format!("Commit agent failed: {}", e));
                                emit_status(SessionStatus::Failed { error: format!("Commit agent failed: {}", e) });
                                emit_event(SessionEventPayload::Finished { reason: "Commit failed".to_string() });
                                return;
                            }
                            Err(e) => {
                                emit_log(LogCategory::Error, format!("Commit agent panicked: {}", e));
                                emit_status(SessionStatus::Failed { error: format!("Commit agent panicked: {}", e) });
                                emit_event(SessionEventPayload::Finished { reason: "Commit failed".to_string() });
                                return;
                            }
                        }
                        continue;
                    }
                    Some(RecoveryAction::HardReset) => {
                        emit_log(LogCategory::Script, "Resetting working tree...".to_string());
                        match git.run_in_worktree(&["reset", "--hard"]).await {
                            Ok(output) => emit_log(LogCategory::Git, output),
                            Err(e) => {
                                emit_log(LogCategory::Error, format!("Reset failed: {}", e));
                                emit_status(SessionStatus::Failed { error: format!("Reset failed: {}", e) });
                                emit_event(SessionEventPayload::Finished { reason: "Reset failed".to_string() });
                                return;
                            }
                        }
                        emit_log(LogCategory::Script, "Retrying...".to_string());
                        continue;
                    }
                    Some(RecoveryAction::Abort) | None => {
                        emit_status(SessionStatus::Failed { error: e });
                        emit_event(SessionEventPayload::Finished { reason: "Stopped by user".to_string() });
                        return;
                    }
                }
            }
            Err(RunError::Transient(e)) => {
                emit_log(
                    LogCategory::Warning,
                    format!("Pre-AI rebase failed: {}. Retrying next iteration...", e),
                );
                continue;
            }
        }

        // Pop stash if we stashed before the rebase
        if stash_pending {
            stash_pending = false;
            emit_log(LogCategory::Script, "Unstashing changes...".to_string());
            match git.run_in_worktree(&["stash", "pop"]).await {
                Ok(output) => emit_log(LogCategory::Git, output),
                Err(e) => {
                    emit_log(LogCategory::Warning, format!("Stash pop failed: {}. Changes remain in stash.", e));
                }
            }
        }

        // Step b: Run AI
        emit_status(SessionStatus::Running {
            step: SessionStep::RunningAi,
            iteration,
        });
        emit_log(
            LogCategory::Script,
            format!("Running {} (iteration {})...", provider.name(), iteration),
        );

        let head_before = match git.get_head().await {
            Ok(h) => h,
            Err(e) => {
                emit_log(LogCategory::Error, format!("Failed to get HEAD: {}", e));
                continue;
            }
        };

        // AI run with crash-recovery retry
        let max_ai_attempts = 3u32;
        let mut ai_ok = false;
        for ai_attempt in 1..=max_ai_attempts {
            let resume_id = current_ai_session_id.clone();
            if ai_attempt > 1 {
                emit_log(
                    LogCategory::Script,
                    format!(
                        "Resuming {} session (attempt {}/{})...",
                        provider.name(), ai_attempt, max_ai_attempts
                    ),
                );
            }

            let (output_tx, mut output_rx) = mpsc::unbounded_channel();
            let abort_clone = abort_rx.clone();
            let working_dir = git.worktree_dir.clone();
            let prompt_clone = match load_prompt(&config.prompt_file, &config.preamble) {
                Ok(p) => p,
                Err(e) => {
                    emit_status(SessionStatus::Failed {
                        error: format!("Failed to load prompt: {}", e),
                    });
                    return;
                }
            };
            let provider_clone = provider.clone();

            let ai_task = tokio::spawn(async move {
                provider_clone
                    .run(&working_dir, &prompt_clone, resume_id.as_deref(), output_tx, abort_clone)
                    .await
            });

            // Forward AI output as log events, capture session ID
            while let Some(output) = output_rx.recv().await {
                match output {
                    AiOutput::Text(text) => {
                        emit_log(LogCategory::Ai, text);
                    }
                    AiOutput::Finished { duration_secs, cost_usd } => {
                        let cost_str = cost_usd
                            .map(|c| format!(" | cost: ${:.4}", c))
                            .unwrap_or_default();
                        emit_log(
                            LogCategory::Script,
                            format!("--- {} finished in {:.1}s{} ---", config.ai_tool, duration_secs, cost_str),
                        );
                    }
                    AiOutput::Error(e) => {
                        emit_log(LogCategory::Error, e);
                    }
                    AiOutput::SessionId(sid) => {
                        current_ai_session_id = Some(sid.clone());
                        emit_event(SessionEventPayload::AiSessionIdChanged {
                            ai_session_id: Some(sid),
                        });
                    }
                }
            }

            match ai_task.await {
                Ok(Ok(())) => {
                    ai_ok = true;
                    break;
                }
                Ok(Err(e)) => {
                    emit_log(LogCategory::Warning, format!("{} failed: {}", config.ai_tool, e));
                    // If we have a session ID, retry with resume
                    if current_ai_session_id.is_some() && ai_attempt < max_ai_attempts {
                        continue;
                    }
                    break;
                }
                Err(e) => {
                    emit_log(LogCategory::Warning, format!("{} task panicked: {}", config.ai_tool, e));
                    break;
                }
            }
        }

        let head_changed = git.head_changed(&head_before).await.unwrap_or(false);
        if !ai_ok || !head_changed {
            emit_log(
                LogCategory::Warning,
                format!("{} made no commits. Skipping housekeeping.", config.ai_tool),
            );
            // Clear session ID — next iteration starts fresh
            current_ai_session_id = None;
            emit_event(SessionEventPayload::AiSessionIdChanged { ai_session_id: None });
            continue;
        }

        // After AI made commits, always do housekeeping
        if *stop_rx.borrow() {
            emit_log(
                LogCategory::Script,
                "Stop requested — pushing commits before exiting...".to_string(),
            );
        }

        // Steps c-g: Git housekeeping
        match git_housekeeping(
            &git,
            &config,
            provider.clone(),
            &emit_log,
            &emit_status,
            &abort_rx,
            iteration,
        )
        .await
        {
            Ok(tag) => {
                // Clear AI session ID — next iteration starts fresh
                current_ai_session_id = None;
                emit_event(SessionEventPayload::AiSessionIdChanged { ai_session_id: None });
                emit_event(SessionEventPayload::IterationComplete {
                    iteration,
                    tag: tag.clone(),
                });
                let timestamp = simple_timestamp();
                if let Some(ref t) = tag {
                    emit_log(
                        LogCategory::Script,
                        format!("=== Iteration {} complete: tagged {} ({}) ===", iteration, t, timestamp),
                    );
                } else {
                    emit_log(
                        LogCategory::Script,
                        format!("=== Iteration {} complete ({}) ===", iteration, timestamp),
                    );
                }
            }
            Err(e) => {
                emit_log(
                    LogCategory::Warning,
                    format!("Git housekeeping failed: {}. Continuing...", e),
                );
            }
        }

    }

    emit_status(SessionStatus::Stopped);
    emit_event(SessionEventPayload::Finished {
        reason: "Stopped".to_string(),
    });
}

async fn rebase_with_conflict_resolution(
    git: &GitOps,
    provider: Arc<dyn AiProvider>,
    emit_log: &impl Fn(LogCategory, String),
    abort_rx: &watch::Receiver<bool>,
) -> Result<(), RunError> {
    match git.rebase_onto_main().await {
        Ok(output) => {
            if !output.trim().is_empty() {
                emit_log(LogCategory::Git, output);
            }
            Ok(())
        }
        Err(RebaseError::Permanent(e)) => {
            // Try self-heal then AI before giving up
            emit_log(LogCategory::Warning, format!("Rebase failed (permanent): {} — attempting recovery", e));
            heal_git_state(git, emit_log).await;
            match git.rebase_onto_main().await {
                Ok(output) => {
                    if !output.trim().is_empty() {
                        emit_log(LogCategory::Git, output);
                    }
                    return Ok(());
                }
                Err(_) => {}
            }
            // Self-heal didn't work — try AI
            recover_with_ai(git, provider.clone(), &e, emit_log, abort_rx).await;
            match git.rebase_onto_main().await {
                Ok(output) => {
                    if !output.trim().is_empty() {
                        emit_log(LogCategory::Git, output);
                    }
                    return Ok(());
                }
                Err(RebaseError::Conflict(c)) => {
                    // AI fixed the permanent issue but now we have conflicts — handle them
                    emit_log(LogCategory::Git, format!("Rebase conflict after recovery: {}", c));
                    // Fall through to conflict handling below
                    // (we can't easily do this with match arms, so abort and return)
                    git.abort_rebase().await.ok();
                    return Err(RunError::Transient(format!("Rebase conflict after recovery: {}", c)));
                }
                Err(RebaseError::Permanent(e2)) => {
                    return Err(RunError::Permanent(e2));
                }
            }
        }
        Err(RebaseError::Conflict(error_output)) => {
            emit_log(LogCategory::Git, format!("Rebase conflict: {}", error_output));

            let max_attempts = 5;
            for attempt in 1..=max_attempts {
                emit_log(
                    LogCategory::Script,
                    format!(
                        "Rebase failed — invoking AI to resolve (attempt {}/{})...",
                        attempt, max_attempts
                    ),
                );

                let conflict_prompt = format!(
                    "A git rebase in this repo failed.\n\n\
                     The error output was:\n{}\n\n\
                     Diagnose the issue from the error above. If there are merge conflicts, \
                     resolve them, stage the files, and run 'git rebase --continue'. \
                     If the error is something else (e.g. unstaged changes, dirty worktree), \
                     fix that first. Do not abort the rebase.",
                    error_output
                );

                let (output_tx, mut output_rx) = mpsc::unbounded_channel();
                let abort_clone = abort_rx.clone();
                let working_dir = git.worktree_dir.clone();
                let provider_clone = provider.clone();

                let resolve_task = tokio::spawn(async move {
                    provider_clone
                        .run(&working_dir, &conflict_prompt, None, output_tx, abort_clone)
                        .await
                });

                while let Some(output) = output_rx.recv().await {
                    if let AiOutput::Text(text) = output {
                        emit_log(LogCategory::Ai, text);
                    }
                }

                resolve_task.await.ok();

                if !git.has_active_rebase() {
                    break;
                }

                if attempt == max_attempts {
                    emit_log(
                        LogCategory::Error,
                        "Failed to resolve rebase after max attempts. Aborting rebase.".to_string(),
                    );
                    git.abort_rebase().await.ok();
                    return Err(RunError::Transient(format!(
                        "Rebase conflict resolution failed after {} attempts (rebase aborted)", max_attempts
                    )));
                }
            }

            if !git.verify_main_is_ancestor().await.unwrap_or(false) {
                return Err(RunError::Permanent(
                    "After rebase, origin/main is not an ancestor of HEAD".to_string(),
                ));
            }

            Ok(())
        }
    }
}

/// Quick self-healing for common git state problems.
/// Removes stale lock files, aborts lingering rebases, and resets dirty index.
async fn heal_git_state(git: &GitOps, emit_log: &impl Fn(LogCategory, String)) {
    git.remove_stale_lock_files(emit_log).await;
    if git.has_active_rebase() {
        emit_log(LogCategory::Warning, "Aborting leftover rebase...".to_string());
        git.abort_rebase().await.ok();
    }
    git.run_in_worktree(&["reset", "--hard"]).await.ok();
}

/// Invoke the AI agent to diagnose and fix a git error.
/// Returns `true` if the agent ran successfully (the error may or may not be fixed).
async fn recover_with_ai(
    git: &GitOps,
    provider: Arc<dyn AiProvider>,
    error_msg: &str,
    emit_log: &impl Fn(LogCategory, String),
    abort_rx: &watch::Receiver<bool>,
) -> bool {
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
        git.worktree_dir.display(),
    );

    let (output_tx, mut output_rx) = mpsc::unbounded_channel();
    let abort_clone = abort_rx.clone();
    let working_dir = git.worktree_dir.clone();
    let provider_clone = provider.clone();

    let task = tokio::spawn(async move {
        provider_clone
            .run(&working_dir, &prompt, None, output_tx, abort_clone)
            .await
    });

    while let Some(output) = output_rx.recv().await {
        match output {
            AiOutput::Text(text) => emit_log(LogCategory::Ai, text),
            AiOutput::Finished { duration_secs, cost_usd } => {
                let cost_str = cost_usd.map(|c| format!(" | cost: ${:.4}", c)).unwrap_or_default();
                emit_log(LogCategory::Script, format!("--- Git recovery agent finished in {:.1}s{} ---", duration_secs, cost_str));
            }
            AiOutput::Error(e) => emit_log(LogCategory::Error, e),
            AiOutput::SessionId(_) => {}
        }
    }

    match task.await {
        Ok(Ok(())) => true,
        Ok(Err(e)) => {
            emit_log(LogCategory::Error, format!("Git recovery agent failed: {}", e));
            false
        }
        Err(e) => {
            emit_log(LogCategory::Error, format!("Git recovery agent panicked: {}", e));
            false
        }
    }
}

/// Full recovery pipeline: heal known issues, then AI agent fallback.
/// Returns `true` if recovery was attempted (caller should retry the operation).
async fn try_git_recovery(
    git: &GitOps,
    _provider: &Arc<dyn AiProvider>,
    error_msg: &str,
    op_name: &str,
    emit_log: &impl Fn(LogCategory, String),
    _abort_rx: &watch::Receiver<bool>,
) -> bool {
    emit_log(
        LogCategory::Warning,
        format!("{} failed: {} — attempting self-heal", op_name, error_msg),
    );
    heal_git_state(git, emit_log).await;

    // Caller will retry after this. If that also fails, caller should call
    // try_git_recovery_ai for the AI fallback.
    true
}

/// AI agent fallback after self-heal didn't work.
async fn try_git_recovery_ai(
    git: &GitOps,
    provider: &Arc<dyn AiProvider>,
    error_msg: &str,
    op_name: &str,
    emit_log: &impl Fn(LogCategory, String),
    abort_rx: &watch::Receiver<bool>,
) -> bool {
    emit_log(
        LogCategory::Warning,
        format!("{} still failing after self-heal: {} — invoking AI recovery agent", op_name, error_msg),
    );
    recover_with_ai(git, provider.clone(), error_msg, emit_log, abort_rx).await
}

async fn git_housekeeping(
    git: &GitOps,
    config: &SessionConfig,
    provider: Arc<dyn AiProvider>,
    emit_log: &impl Fn(LogCategory, String),
    emit_status: &impl Fn(SessionStatus),
    abort_rx: &watch::Receiver<bool>,
    iteration: u32,
) -> anyhow::Result<Option<String>> {
    // Step c: Push branch (with recovery)
    emit_status(SessionStatus::Running {
        step: SessionStep::PushBranch,
        iteration,
    });
    emit_log(LogCategory::Git, "Pushing branch to origin...".to_string());
    if let Err(e) = git.push_branch().await {
        try_git_recovery(git, &provider, &format!("{}", e), "Push branch", emit_log, abort_rx).await;
        if let Err(e2) = git.push_branch().await {
            try_git_recovery_ai(git, &provider, &format!("{}", e2), "Push branch", emit_log, abort_rx).await;
            if let Err(e3) = git.push_branch().await {
                return Err(e3);
            }
        }
    }

    // Step d: Rebase onto main again (with recovery)
    emit_status(SessionStatus::Running {
        step: SessionStep::RebasePostAi,
        iteration,
    });
    if let Err(e) = git.fetch_main().await {
        emit_log(LogCategory::Warning, format!("Fetch failed: {} — attempting recovery", e));
        try_git_recovery(git, &provider, &format!("{}", e), "Fetch main", emit_log, abort_rx).await;
        if let Err(e2) = git.fetch_main().await {
            try_git_recovery_ai(git, &provider, &format!("{}", e2), "Fetch main", emit_log, abort_rx).await;
            if let Err(e3) = git.fetch_main().await {
                emit_log(LogCategory::Warning, format!("Fetch still failing: {}. Proceeding with stale main.", e3));
            }
        }
    }
    if let Err(e) = rebase_with_conflict_resolution(git, provider.clone(), emit_log, abort_rx).await {
        // Clean up rebase/merge state so the next iteration doesn't get stuck
        heal_git_state(git, emit_log).await;
        return Err(anyhow::anyhow!("{}", e));
    }

    // Log diff stat
    if let Ok(diff_stat) = git.diff_stat_against_main().await {
        if !diff_stat.trim().is_empty() {
            emit_log(LogCategory::Script, format!("--- Files pushed to {} ---", config.main_branch));
            emit_log(LogCategory::Git, diff_stat);
        }
    }

    // Step e: Push to main (with recovery)
    emit_status(SessionStatus::Running {
        step: SessionStep::PushToMain,
        iteration,
    });
    emit_log(LogCategory::Git, "Pushing to main...".to_string());
    if let Err(e) = git.push_to_main().await {
        try_git_recovery(git, &provider, &format!("{}", e), "Push to main", emit_log, abort_rx).await;
        if let Err(e2) = git.push_to_main().await {
            try_git_recovery_ai(git, &provider, &format!("{}", e2), "Push to main", emit_log, abort_rx).await;
            if let Err(e3) = git.push_to_main().await {
                return Err(e3);
            }
        }
    }

    // Steps f+g: Tag and push
    let mut tag = None;
    if config.tagging_enabled {
        emit_status(SessionStatus::Running {
            step: SessionStep::Tagging,
            iteration,
        });
        emit_log(LogCategory::Git, "Creating and pushing tag...".to_string());
        match git.tag_and_push().await {
            Ok(new_tag) => {
                emit_log(LogCategory::Git, format!("Tagged {}", new_tag));
                tag = Some(new_tag);
            }
            Err(e) => {
                emit_log(LogCategory::Warning, format!("Tagging failed: {}", e));
            }
        }
    }

    Ok(tag)
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
