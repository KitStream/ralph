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

    // Load prompt
    let prompt = match load_prompt(&config.prompt_file, &config.preamble) {
        Ok(p) => p,
        Err(e) => {
            emit_status(SessionStatus::Failed {
                error: format!("Failed to load prompt: {}", e),
            });
            return;
        }
    };

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

    loop {
        if *stop_rx.borrow() {
            emit_log(LogCategory::Script, "Stop requested. Exiting.".to_string());
            break;
        }

        iteration += 1;

        // Step 0: Checkout
        emit_status(SessionStatus::Running {
            step: SessionStep::Checkout,
            iteration,
        });
        if let Err(e) = git.checkout_branch().await {
            emit_log(LogCategory::Warning, format!("Checkout failed: {}", e));
            continue;
        }

        if *stop_rx.borrow() {
            emit_log(LogCategory::Script, "Stop requested. Exiting.".to_string());
            break;
        }

        // Step a: Rebase onto main
        emit_status(SessionStatus::Running {
            step: SessionStep::RebasePreAi,
            iteration,
        });
        emit_log(LogCategory::Git, "Fetching and rebasing onto main...".to_string());

        if let Err(e) = git.fetch_main().await {
            emit_log(LogCategory::Warning, format!("Fetch failed: {}", e));
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
                        emit_log(LogCategory::Script, "Retrying...".to_string());
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
                                .run(&working_dir, commit_prompt, output_tx, abort_clone)
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

        if *stop_rx.borrow() {
            emit_log(LogCategory::Script, "Stop requested. Exiting.".to_string());
            break;
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

        let (output_tx, mut output_rx) = mpsc::unbounded_channel();
        let abort_clone = abort_rx.clone();
        let working_dir = git.worktree_dir.clone();
        let prompt_clone = prompt.clone();
        let provider_clone = provider.clone();

        let ai_task = tokio::spawn(async move {
            provider_clone
                .run(&working_dir, &prompt_clone, output_tx, abort_clone)
                .await
        });

        // Forward AI output as log events
        while let Some(output) = output_rx.recv().await {
            match output {
                AiOutput::Text(text) => {
                    emit_log(LogCategory::Ai, text);
                }
                AiOutput::Finished {
                    duration_secs,
                    cost_usd,
                } => {
                    let cost_str = cost_usd
                        .map(|c| format!(" | cost: ${:.4}", c))
                        .unwrap_or_default();
                    emit_log(
                        LogCategory::Script,
                        format!(
                            "--- {} finished in {:.1}s{} ---",
                            config.ai_tool, duration_secs, cost_str
                        ),
                    );
                }
                AiOutput::Error(e) => {
                    emit_log(LogCategory::Error, e);
                }
            }
        }

        let ai_ok = match ai_task.await {
            Ok(Ok(())) => true,
            Ok(Err(e)) => {
                emit_log(
                    LogCategory::Warning,
                    format!("{} failed: {}", config.ai_tool, e),
                );
                false
            }
            Err(e) => {
                emit_log(
                    LogCategory::Warning,
                    format!("{} task panicked: {}", config.ai_tool, e),
                );
                false
            }
        };

        let head_changed = git.head_changed(&head_before).await.unwrap_or(false);
        if !ai_ok || !head_changed {
            emit_log(
                LogCategory::Warning,
                format!("{} made no commits. Skipping housekeeping.", config.ai_tool),
            );
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

        if *stop_rx.borrow() {
            emit_log(LogCategory::Script, "Stop requested. Exiting.".to_string());
            break;
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
            Err(RunError::Permanent(e))
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
                        .run(&working_dir, &conflict_prompt, output_tx, abort_clone)
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

async fn git_housekeeping(
    git: &GitOps,
    config: &SessionConfig,
    provider: Arc<dyn AiProvider>,
    emit_log: &impl Fn(LogCategory, String),
    emit_status: &impl Fn(SessionStatus),
    abort_rx: &watch::Receiver<bool>,
    iteration: u32,
) -> anyhow::Result<Option<String>> {
    // Step c: Push branch
    emit_status(SessionStatus::Running {
        step: SessionStep::PushBranch,
        iteration,
    });
    emit_log(LogCategory::Git, "Pushing branch to origin...".to_string());
    match git.push_branch().await {
        Ok(output) => {
            if !output.trim().is_empty() {
                emit_log(LogCategory::Git, output);
            }
        }
        Err(e) => return Err(e),
    }

    // Step d: Rebase onto main again
    emit_status(SessionStatus::Running {
        step: SessionStep::RebasePostAi,
        iteration,
    });
    if let Err(e) = git.fetch_main().await {
        emit_log(LogCategory::Warning, format!("Fetch failed: {}", e));
    }
    rebase_with_conflict_resolution(git, provider, emit_log, abort_rx)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Log diff stat
    if let Ok(diff_stat) = git.diff_stat_against_main().await {
        if !diff_stat.trim().is_empty() {
            emit_log(LogCategory::Script, format!("--- Files pushed to {} ---", config.main_branch));
            emit_log(LogCategory::Git, diff_stat);
        }
    }

    // Step e: Push to main
    emit_status(SessionStatus::Running {
        step: SessionStep::PushToMain,
        iteration,
    });
    emit_log(LogCategory::Git, "Pushing to main...".to_string());
    match git.push_to_main().await {
        Ok(output) => {
            if !output.trim().is_empty() {
                emit_log(LogCategory::Git, output);
            }
        }
        Err(e) => return Err(e),
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
