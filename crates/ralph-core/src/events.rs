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
}

/// Canonical representation of tool invocations.
/// Each variant fully describes the tool input in a typed way.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool")]
pub enum ToolInvocation {
    Read {
        file_path: String,
    },
    Edit {
        file_path: String,
        old_string: String,
        new_string: String,
    },
    Write {
        file_path: String,
        content: String,
    },
    Bash {
        command: String,
        description: Option<String>,
    },
    Glob {
        pattern: String,
        path: Option<String>,
    },
    Grep {
        pattern: String,
        path: Option<String>,
        include: Option<String>,
    },
    /// Catch-all for tools we don't have specific rendering for.
    Other {
        name: String,
        input: serde_json::Value,
    },
}

/// Structured housekeeping event for git operations and session lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum HousekeepingBlock {
    /// A named step started (checkout, rebase, push, tag).
    StepStarted {
        step: SessionStep,
        description: String,
    },
    /// A step completed.
    StepCompleted { step: SessionStep, summary: String },
    /// Git command output.
    GitCommand {
        command: String,
        output: String,
        success: bool,
    },
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
    Prompt,
}

/// Callback type for emitting events. Both CLI and GUI provide their own implementation.
pub type EventCallback = Box<dyn Fn(SessionEvent) + Send + Sync>;

/// Tool result data, extracted from ToolResult events for attachment to ToolUse entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultData {
    pub content: String,
    pub is_error: bool,
}

impl AiContentBlock {
    pub fn summary(&self) -> String {
        match self {
            AiContentBlock::Text { text } => text.clone(),
            AiContentBlock::ToolUse { tool, .. } => match tool {
                ToolInvocation::Read { file_path } => format!("Read {}", file_path),
                ToolInvocation::Edit { file_path, .. } => format!("Edit {}", file_path),
                ToolInvocation::Write { file_path, .. } => format!("Write {}", file_path),
                ToolInvocation::Bash { command, .. } => format!("$ {}", command),
                ToolInvocation::Glob { pattern, .. } => format!("Glob {}", pattern),
                ToolInvocation::Grep { pattern, .. } => format!("Grep {}", pattern),
                ToolInvocation::Other { name, .. } => name.clone(),
            },
            AiContentBlock::ToolResult { content, .. } => {
                if content.len() > 200 {
                    content[..200].to_string()
                } else {
                    content.clone()
                }
            }
        }
    }
}

/// Replace all occurrences of a worktree prefix in `text` with `⌂`.
/// Uses OS-appropriate case sensitivity (case-insensitive on macOS/Windows,
/// case-sensitive on Linux) and normalizes path separators.
pub fn shorten_paths(text: &str, prefix: &str) -> String {
    if prefix.is_empty() {
        return text.to_string();
    }
    let norm_prefix: String = prefix.replace('\\', "/");
    // Trim trailing slash from prefix for matching
    let norm_prefix = norm_prefix.trim_end_matches('/');
    if norm_prefix.is_empty() {
        return text.to_string();
    }

    let norm_text: String = text.replace('\\', "/");
    let mut result = String::with_capacity(text.len());
    let mut i = 0;

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let (cmp_text, cmp_prefix) = (norm_text.to_lowercase(), norm_prefix.to_lowercase());
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let (cmp_text, cmp_prefix) = (norm_text.clone(), norm_prefix.to_string());

    while i < text.len() {
        if cmp_text[i..].starts_with(&cmp_prefix) {
            result.push('\u{2302}');
            i += norm_prefix.len();
        } else {
            // Use the original character (preserve original casing/separators)
            result.push(text.as_bytes()[i] as char);
            i += 1;
        }
    }
    result
}

impl ToolInvocation {
    /// Return a clone with all path fields shortened.
    pub fn with_short_paths(&self, prefix: &str) -> Self {
        let sp = |p: &str| shorten_paths(p, prefix);
        match self {
            ToolInvocation::Read { file_path } => ToolInvocation::Read {
                file_path: sp(file_path),
            },
            ToolInvocation::Edit {
                file_path,
                old_string,
                new_string,
            } => ToolInvocation::Edit {
                file_path: sp(file_path),
                old_string: old_string.clone(),
                new_string: new_string.clone(),
            },
            ToolInvocation::Write { file_path, content } => ToolInvocation::Write {
                file_path: sp(file_path),
                content: content.clone(),
            },
            ToolInvocation::Bash {
                command,
                description,
            } => ToolInvocation::Bash {
                command: sp(command),
                description: description.clone(),
            },
            ToolInvocation::Glob { pattern, path } => ToolInvocation::Glob {
                pattern: pattern.clone(),
                path: path.as_ref().map(|p| sp(p)),
            },
            ToolInvocation::Grep {
                pattern,
                path,
                include,
            } => ToolInvocation::Grep {
                pattern: pattern.clone(),
                path: path.as_ref().map(|p| sp(p)),
                include: include.clone(),
            },
            ToolInvocation::Other { name, input } => ToolInvocation::Other {
                name: name.clone(),
                input: input.clone(),
            },
        }
    }
}

impl AiContentBlock {
    /// Return a clone with all tool paths shortened.
    pub fn with_short_paths(&self, prefix: &str) -> Self {
        match self {
            AiContentBlock::ToolUse { tool_id, tool } => AiContentBlock::ToolUse {
                tool_id: tool_id.clone(),
                tool: tool.with_short_paths(prefix),
            },
            other => other.clone(),
        }
    }
}

impl HousekeepingBlock {
    pub fn summary(&self) -> String {
        match self {
            HousekeepingBlock::StepStarted { description, .. } => {
                format!("\u{25b8} {}", description)
            }
            HousekeepingBlock::StepCompleted { summary, .. } => format!("\u{2713} {}", summary),
            HousekeepingBlock::GitCommand { output, .. } => output.clone(),
            HousekeepingBlock::DiffStat { stat } => stat.clone(),
            HousekeepingBlock::Recovery { action, detail } => format!("{}: {}", action, detail),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::state::SessionStep;

    #[test]
    fn ai_content_text_summary_returns_text() {
        let block = AiContentBlock::Text {
            text: "hello world".to_string(),
        };
        assert_eq!(block.summary(), "hello world");
    }

    #[test]
    fn ai_content_tool_use_read_summary() {
        let block = AiContentBlock::ToolUse {
            tool_id: "t1".to_string(),
            tool: ToolInvocation::Read {
                file_path: "/src/main.rs".to_string(),
            },
        };
        assert_eq!(block.summary(), "Read /src/main.rs");
    }

    #[test]
    fn ai_content_tool_use_edit_summary() {
        let block = AiContentBlock::ToolUse {
            tool_id: "t2".to_string(),
            tool: ToolInvocation::Edit {
                file_path: "/src/lib.rs".to_string(),
                old_string: "old".to_string(),
                new_string: "new".to_string(),
            },
        };
        assert_eq!(block.summary(), "Edit /src/lib.rs");
    }

    #[test]
    fn ai_content_tool_use_write_summary() {
        let block = AiContentBlock::ToolUse {
            tool_id: "t3".to_string(),
            tool: ToolInvocation::Write {
                file_path: "/tmp/out.txt".to_string(),
                content: "data".to_string(),
            },
        };
        assert_eq!(block.summary(), "Write /tmp/out.txt");
    }

    #[test]
    fn ai_content_tool_use_bash_summary() {
        let block = AiContentBlock::ToolUse {
            tool_id: "t4".to_string(),
            tool: ToolInvocation::Bash {
                command: "cargo test".to_string(),
                description: None,
            },
        };
        assert_eq!(block.summary(), "$ cargo test");
    }

    #[test]
    fn ai_content_tool_use_glob_summary() {
        let block = AiContentBlock::ToolUse {
            tool_id: "t5".to_string(),
            tool: ToolInvocation::Glob {
                pattern: "**/*.rs".to_string(),
                path: None,
            },
        };
        assert_eq!(block.summary(), "Glob **/*.rs");
    }

    #[test]
    fn ai_content_tool_use_grep_summary() {
        let block = AiContentBlock::ToolUse {
            tool_id: "t6".to_string(),
            tool: ToolInvocation::Grep {
                pattern: "TODO".to_string(),
                path: None,
                include: None,
            },
        };
        assert_eq!(block.summary(), "Grep TODO");
    }

    #[test]
    fn ai_content_tool_use_other_summary() {
        let block = AiContentBlock::ToolUse {
            tool_id: "t7".to_string(),
            tool: ToolInvocation::Other {
                name: "CustomTool".to_string(),
                input: serde_json::json!({}),
            },
        };
        assert_eq!(block.summary(), "CustomTool");
    }

    #[test]
    fn ai_content_tool_result_truncates_long_content() {
        let long_content = "x".repeat(300);
        let block = AiContentBlock::ToolResult {
            tool_use_id: "t1".to_string(),
            content: long_content,
            is_error: false,
        };
        assert_eq!(block.summary().len(), 200);
    }

    #[test]
    fn ai_content_tool_result_short_content_unchanged() {
        let block = AiContentBlock::ToolResult {
            tool_use_id: "t1".to_string(),
            content: "short".to_string(),
            is_error: false,
        };
        assert_eq!(block.summary(), "short");
    }

    #[test]
    fn housekeeping_step_started_summary() {
        let block = HousekeepingBlock::StepStarted {
            step: SessionStep::Checkout,
            description: "Checking out branch".to_string(),
        };
        assert_eq!(block.summary(), "\u{25b8} Checking out branch");
    }

    #[test]
    fn housekeeping_step_completed_summary() {
        let block = HousekeepingBlock::StepCompleted {
            step: SessionStep::PushBranch,
            summary: "Branch pushed".to_string(),
        };
        assert_eq!(block.summary(), "\u{2713} Branch pushed");
    }

    #[test]
    fn housekeeping_git_command_summary() {
        let block = HousekeepingBlock::GitCommand {
            command: "git status".to_string(),
            output: "On branch main".to_string(),
            success: true,
        };
        assert_eq!(block.summary(), "On branch main");
    }

    #[test]
    fn housekeeping_diff_stat_summary() {
        let block = HousekeepingBlock::DiffStat {
            stat: " 3 files changed".to_string(),
        };
        assert_eq!(block.summary(), " 3 files changed");
    }

    #[test]
    fn housekeeping_recovery_summary() {
        let block = HousekeepingBlock::Recovery {
            action: "Stash".to_string(),
            detail: "saved changes".to_string(),
        };
        assert_eq!(block.summary(), "Stash: saved changes");
    }

    // ── shorten_paths tests ──────────────────────────────────────────

    #[test]
    fn shorten_paths_empty_prefix_returns_unchanged() {
        assert_eq!(shorten_paths("some/path/file.rs", ""), "some/path/file.rs");
    }

    #[test]
    fn shorten_paths_empty_text_returns_empty() {
        assert_eq!(
            shorten_paths("", "/home/user/project/.ralph/branch-worktree"),
            ""
        );
    }

    #[test]
    fn shorten_paths_exact_match_produces_home() {
        let prefix = "/home/user/project/.ralph/branch-worktree";
        assert_eq!(shorten_paths(prefix, prefix), "\u{2302}");
    }

    #[test]
    fn shorten_paths_prefix_at_start_of_path() {
        assert_eq!(
            shorten_paths(
                "/home/user/project/.ralph/branch-worktree/src/main.rs",
                "/home/user/project/.ralph/branch-worktree"
            ),
            "\u{2302}/src/main.rs"
        );
    }

    #[test]
    fn shorten_paths_multiple_occurrences() {
        let prefix = "/home/user/project/.ralph/branch-worktree";
        let text = format!("cat {}/a.txt {}/b.txt", prefix, prefix);
        assert_eq!(
            shorten_paths(&text, prefix),
            "cat \u{2302}/a.txt \u{2302}/b.txt"
        );
    }

    #[test]
    fn shorten_paths_trailing_slash_stripped() {
        assert_eq!(
            shorten_paths(
                "/home/user/project/.ralph/branch-worktree/src/main.rs",
                "/home/user/project/.ralph/branch-worktree/"
            ),
            "\u{2302}/src/main.rs"
        );
    }

    #[test]
    fn shorten_paths_backslash_normalization() {
        // Both text and prefix use backslashes; the function normalizes
        // separators internally for matching, but preserves original chars
        // in the non-matched portions of the output.
        assert_eq!(
            shorten_paths(
                "C:\\Users\\foo\\project\\.ralph\\wt\\src\\main.rs",
                "C:\\Users\\foo\\project\\.ralph\\wt"
            ),
            "\u{2302}\\src\\main.rs"
        );
    }

    #[test]
    fn shorten_paths_mixed_separators() {
        // Prefix uses forward slashes, text uses forward slashes too
        assert_eq!(
            shorten_paths(
                "C:/Users/foo/project/.ralph/wt/src/main.rs",
                "C:/Users/foo/project/.ralph/wt"
            ),
            "\u{2302}/src/main.rs"
        );
    }

    #[test]
    fn shorten_paths_no_match_unchanged() {
        let prefix = "/home/user/project/.ralph/branch-worktree";
        assert_eq!(
            shorten_paths("/other/path/file.rs", prefix),
            "/other/path/file.rs"
        );
    }

    #[test]
    fn shorten_paths_partial_prefix_match() {
        let prefix = "/home/user/project/.ralph/branch-worktree";
        assert_eq!(
            shorten_paths(
                "/home/user/project/.ralph/branch-worktree-extra/file.rs",
                prefix
            ),
            "\u{2302}-extra/file.rs"
        );
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn shorten_paths_case_insensitive_on_macos() {
        assert_eq!(
            shorten_paths("/users/foo/project/file.rs", "/Users/Foo/Project"),
            "\u{2302}/file.rs"
        );
    }

    // ── ToolInvocation::with_short_paths tests ──────────────────────

    #[test]
    fn tool_invocation_read_shortens_file_path() {
        let prefix = "/home/user/project/.ralph/wt";
        let tool = ToolInvocation::Read {
            file_path: format!("{}/src/main.rs", prefix),
        };
        match tool.with_short_paths(prefix) {
            ToolInvocation::Read { file_path } => {
                assert_eq!(file_path, "\u{2302}/src/main.rs");
            }
            _ => panic!("expected Read"),
        }
    }

    #[test]
    fn tool_invocation_edit_shortens_file_path_not_strings() {
        let prefix = "/home/user/project/.ralph/wt";
        let tool = ToolInvocation::Edit {
            file_path: format!("{}/src/lib.rs", prefix),
            old_string: format!("{}/old", prefix),
            new_string: format!("{}/new", prefix),
        };
        match tool.with_short_paths(prefix) {
            ToolInvocation::Edit {
                file_path,
                old_string,
                new_string,
            } => {
                assert_eq!(file_path, "\u{2302}/src/lib.rs");
                assert_eq!(old_string, format!("{}/old", prefix));
                assert_eq!(new_string, format!("{}/new", prefix));
            }
            _ => panic!("expected Edit"),
        }
    }

    #[test]
    fn tool_invocation_bash_shortens_command() {
        let prefix = "/home/user/project/.ralph/wt";
        let tool = ToolInvocation::Bash {
            command: format!("ls {}/src", prefix),
            description: None,
        };
        match tool.with_short_paths(prefix) {
            ToolInvocation::Bash { command, .. } => {
                assert_eq!(command, "ls \u{2302}/src");
            }
            _ => panic!("expected Bash"),
        }
    }

    #[test]
    fn tool_invocation_glob_shortens_path_not_pattern() {
        let prefix = "/home/user/project/.ralph/wt";
        let tool = ToolInvocation::Glob {
            pattern: "**/*.rs".to_string(),
            path: Some(format!("{}/src", prefix)),
        };
        match tool.with_short_paths(prefix) {
            ToolInvocation::Glob { pattern, path } => {
                assert_eq!(pattern, "**/*.rs");
                assert_eq!(path.unwrap(), "\u{2302}/src");
            }
            _ => panic!("expected Glob"),
        }
    }

    #[test]
    fn tool_invocation_other_unchanged() {
        let prefix = "/home/user/project/.ralph/wt";
        let tool = ToolInvocation::Other {
            name: "CustomTool".to_string(),
            input: serde_json::json!({"key": format!("{}/val", prefix)}),
        };
        match tool.with_short_paths(prefix) {
            ToolInvocation::Other { name, input } => {
                assert_eq!(name, "CustomTool");
                assert_eq!(input["key"], format!("{}/val", prefix));
            }
            _ => panic!("expected Other"),
        }
    }

    // ── AiContentBlock::with_short_paths tests ─────────────────────

    #[test]
    fn ai_block_tool_use_shortens_paths() {
        let prefix = "/home/user/project/.ralph/wt";
        let block = AiContentBlock::ToolUse {
            tool_id: "t1".to_string(),
            tool: ToolInvocation::Read {
                file_path: format!("{}/src/main.rs", prefix),
            },
        };
        match block.with_short_paths(prefix) {
            AiContentBlock::ToolUse { tool, .. } => match tool {
                ToolInvocation::Read { file_path } => {
                    assert_eq!(file_path, "\u{2302}/src/main.rs");
                }
                _ => panic!("expected Read"),
            },
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn ai_block_text_unchanged() {
        let prefix = "/home/user/project/.ralph/wt";
        let block = AiContentBlock::Text {
            text: format!("path: {}/foo", prefix),
        };
        match block.with_short_paths(prefix) {
            AiContentBlock::Text { text } => {
                assert_eq!(text, format!("path: {}/foo", prefix));
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn ai_block_tool_result_unchanged() {
        let prefix = "/home/user/project/.ralph/wt";
        let block = AiContentBlock::ToolResult {
            tool_use_id: "t1".to_string(),
            content: format!("{}/foo", prefix),
            is_error: false,
        };
        match block.with_short_paths(prefix) {
            AiContentBlock::ToolResult { content, .. } => {
                assert_eq!(content, format!("{}/foo", prefix));
            }
            _ => panic!("expected ToolResult"),
        }
    }
}
