use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use clap::Parser;
use colored::Colorize;

mod update_check;

use ralph_core::discovery::discover_modes;
use ralph_core::events::{
    AiContentBlock, HousekeepingBlock, LogCategory, RecoveryAction, SessionEvent,
    SessionEventPayload, ToolInvocation,
};
use ralph_core::provider::AiTool;
use ralph_core::session::runner::run_session;
use ralph_core::session::state::{SessionConfig, SessionId};

#[derive(Parser)]
#[command(name = "ralph", about = "Autonomous coding loop")]
struct Cli {
    /// Mode (derived from PROMPT-<mode>.md files)
    mode: String,

    /// Branch name (default: "ralph-<mode>")
    #[arg(short, long)]
    branch: Option<String>,

    /// Main/common branch name
    #[arg(short, long, default_value = "main")]
    main_branch: String,

    /// Preamble text prepended to the prompt
    #[arg(short, long, default_value = "")]
    preamble: String,

    /// Disable automatic semver tagging
    #[arg(short = 'T', long)]
    no_tag: bool,

    /// AI backend to use
    #[arg(short = 'B', long, default_value = "claude")]
    backend: AiTool,

    /// Model to use (backend-specific, e.g. "sonnet", "opus", "o3")
    #[arg(short = 'm', long)]
    model: Option<String>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Kick off an update check in parallel with session setup. The check has
    // a short timeout and swallows its own errors, so it never blocks startup.
    let update_task = tokio::spawn(update_check::check_and_notify(env!("CARGO_PKG_VERSION")));

    let cwd = std::env::current_dir().expect("Failed to get current directory");
    let modes = discover_modes(&[cwd.as_path()]);

    let mode_info = modes.iter().find(|m| m.name == cli.mode);
    let mode_info = match mode_info {
        Some(m) => m,
        None => {
            eprintln!("{} Unknown mode: {}", "ERROR:".red().bold(), cli.mode);
            if modes.is_empty() {
                eprintln!("No PROMPT-<mode>.md files found in {:?}", cwd);
            } else {
                eprintln!(
                    "Available modes: {}",
                    modes
                        .iter()
                        .map(|m| m.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            std::process::exit(1);
        }
    };

    let branch_name = cli.branch.unwrap_or_else(|| format!("ralph-{}", cli.mode));

    let config = SessionConfig {
        project_dir: cwd.canonicalize().unwrap_or_else(|_| cwd.clone()),
        mode: cli.mode.clone(),
        prompt_file: mode_info.prompt_file.clone(),
        branch_name,
        main_branch: cli.main_branch,
        preamble: cli.preamble,
        tagging_enabled: !cli.no_tag,
        ai_tool: cli.backend,
        model: cli.model,
    };

    let id = SessionId::new();

    // Handle Ctrl+C: first = graceful stop, second = abort
    let ctrl_c_count = Arc::new(AtomicU8::new(0));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let abort_flag = Arc::new(AtomicBool::new(false));

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let (abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let (action_tx, action_rx) = tokio::sync::mpsc::channel::<RecoveryAction>(1);

    {
        let ctrl_c_count = ctrl_c_count.clone();
        let stop_flag = stop_flag.clone();
        let abort_flag = abort_flag.clone();
        ctrlc_handler(move || {
            let count = ctrl_c_count.fetch_add(1, Ordering::SeqCst);
            if count == 0 {
                eprintln!(
                    "{}",
                    "Interrupted, will stop after current iteration. Press Ctrl+C again to abort immediately."
                        .yellow()
                        .bold()
                );
                stop_flag.store(true, Ordering::SeqCst);
                stop_tx.send(true).ok();
            } else {
                eprintln!(
                    "{}",
                    "Second Ctrl+C — aborting immediately.".yellow().bold()
                );
                abort_flag.store(true, Ordering::SeqCst);
                abort_tx.send(true).ok();
            }
        });
    }

    println!(
        "{}",
        format!(
            "Running in mode: {} (branch: {}, backend: {})",
            config.mode, config.branch_name, config.ai_tool
        )
        .magenta()
    );
    println!(
        "{}",
        "Press Ctrl+C to stop after the current iteration, twice to abort immediately.".magenta()
    );

    run_session(
        id,
        config,
        move |event: SessionEvent| {
            print_event(&event);
            // In CLI mode, auto-respond to recovery prompts with Stash
            if let SessionEventPayload::ActionRequired { .. } = &event.payload {
                let tx = action_tx.clone();
                tokio::spawn(async move {
                    eprintln!("{}", "Auto-recovering: stashing changes...".yellow());
                    tx.send(RecoveryAction::Stash).await.ok();
                });
            }
        },
        stop_rx,
        abort_rx,
        action_rx,
        None, // No resume for CLI fresh start
        None, // No resume step
        None, // No resume iteration
    )
    .await;

    // Give the update check a final moment to report before we exit.
    let _ = tokio::time::timeout(std::time::Duration::from_millis(100), update_task).await;

    if abort_flag.load(Ordering::SeqCst) {
        std::process::exit(130);
    }
}

fn print_event(event: &SessionEvent) {
    match &event.payload {
        SessionEventPayload::Log { category, text } => {
            let colored_text = match category {
                LogCategory::Git => text.cyan().to_string(),
                LogCategory::Ai => text.green().to_string(),
                LogCategory::Script => text.magenta().to_string(),
                LogCategory::Warning => text.yellow().to_string(),
                LogCategory::Error => text.red().to_string(),
                LogCategory::Prompt => text.dimmed().to_string(),
            };
            println!("{}", colored_text);
        }
        SessionEventPayload::AiContent { block } => {
            print_ai_content(block);
        }
        SessionEventPayload::Housekeeping { block } => {
            print_housekeeping(block);
        }
        SessionEventPayload::StatusChanged { status } => {
            let _ = status;
        }
        SessionEventPayload::IterationComplete { iteration, tag } => {
            let msg = if let Some(tag) = tag {
                format!("=== Iteration {} complete: tagged {} ===", iteration, tag)
            } else {
                format!("=== Iteration {} complete ===", iteration)
            };
            println!("{}", msg.magenta());
        }
        SessionEventPayload::Finished { reason } => {
            println!("{}", format!("Session finished: {}", reason).magenta());
        }
        SessionEventPayload::RateLimited { message } => {
            eprintln!("{}", format!("⏸ Rate limited: {}", message).yellow().bold());
            eprintln!("{}", "  Waiting for limit to reset...".yellow());
        }
        SessionEventPayload::ActionRequired { error, .. } => {
            eprintln!("{}", format!("Recovery needed: {}", error).yellow().bold());
        }
        SessionEventPayload::AiSessionIdChanged { .. } => {}
    }
}

fn print_ai_content(block: &AiContentBlock) {
    match block {
        AiContentBlock::Text { text } => {
            println!("{}", text.green());
        }
        AiContentBlock::ToolUse { tool, .. } => match tool {
            ToolInvocation::Read { file_path } => {
                println!("  {} {}", "Read".blue().bold(), file_path.white());
            }
            ToolInvocation::Edit {
                file_path,
                old_string,
                new_string,
            } => {
                println!("  {} {}", "Edit".yellow().bold(), file_path.white());
                for line in old_string.lines() {
                    println!("    {}{}", "- ".red(), line.red());
                }
                for line in new_string.lines() {
                    println!("    {}{}", "+ ".green(), line.green());
                }
            }
            ToolInvocation::Write { file_path, .. } => {
                println!("  {} {}", "Write".yellow().bold(), file_path.white());
            }
            ToolInvocation::Bash {
                command,
                description,
            } => {
                println!("  {} {}", "$".cyan().bold(), command.white());
                if let Some(desc) = description {
                    println!("    {}", desc.dimmed());
                }
            }
            ToolInvocation::Glob { pattern, path } => {
                let suffix = path
                    .as_deref()
                    .map(|p| format!(" in {}", p))
                    .unwrap_or_default();
                println!(
                    "  {} {}{}",
                    "Glob".blue().bold(),
                    pattern.white(),
                    suffix.dimmed()
                );
            }
            ToolInvocation::Grep { pattern, path, .. } => {
                let suffix = path
                    .as_deref()
                    .map(|p| format!(" in {}", p))
                    .unwrap_or_default();
                println!(
                    "  {} {}{}",
                    "Grep".blue().bold(),
                    pattern.white(),
                    suffix.dimmed()
                );
            }
            ToolInvocation::Other { name, .. } => {
                println!("  {} {}", "Tool".blue().bold(), name.white());
            }
        },
        AiContentBlock::ToolResult {
            content, is_error, ..
        } => {
            let max_lines = 10;
            let total_lines = content.lines().count();
            for line in content.lines().take(max_lines) {
                if *is_error {
                    println!("    {}", line.red());
                } else {
                    println!("    {}", line.dimmed());
                }
            }
            if total_lines > max_lines {
                println!(
                    "    {}",
                    format!("... ({} more lines)", total_lines - max_lines).dimmed()
                );
            }
        }
    }
}

fn print_housekeeping(block: &HousekeepingBlock) {
    match block {
        HousekeepingBlock::StepStarted { step, description } => {
            println!(
                "{}",
                format!("▸ [{}] {}", step_label(step), description).cyan()
            );
        }
        HousekeepingBlock::StepCompleted { step, summary } => {
            println!("{}", format!("✓ [{}] {}", step_label(step), summary).cyan());
        }
        HousekeepingBlock::GitCommand { output, .. } => {
            if !output.trim().is_empty() {
                println!("{}", output.cyan());
            }
        }
        HousekeepingBlock::DiffStat { stat } => {
            println!("{}", stat.cyan());
        }
        HousekeepingBlock::Recovery { action, detail } => {
            println!("{}", format!("↻ {}: {}", action, detail).yellow());
        }
    }
}

fn step_label(step: &ralph_core::session::state::SessionStep) -> &'static str {
    use ralph_core::session::state::SessionStep;
    match step {
        SessionStep::Idle => "idle",
        SessionStep::Checkout => "checkout",
        SessionStep::RebasePreAi => "rebase",
        SessionStep::RunningAi => "ai",
        SessionStep::PushBranch => "push",
        SessionStep::RebasePostAi => "rebase",
        SessionStep::PushToMain => "push-main",
        SessionStep::Tagging => "tag",
        SessionStep::RecoveringGit => "recovery",
        SessionStep::Paused => "paused",
    }
}

fn ctrlc_handler(f: impl Fn() + Send + 'static) {
    // Use a simple signal handler
    let f = Arc::new(std::sync::Mutex::new(f));
    tokio::spawn(async move {
        loop {
            tokio::signal::ctrl_c().await.ok();
            let f = f.lock().unwrap();
            f();
        }
    });
}
