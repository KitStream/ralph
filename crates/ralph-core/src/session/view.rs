use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::events::{
    shorten_paths, AiContentBlock, HousekeepingBlock, LogCategory, SessionEventPayload,
    ToolResultData,
};
use crate::session::log_store::LogRecord;

/// A pre-processed log entry ready for rendering. Tool results are already
/// attached to their corresponding tool-use entries, and summaries are computed.
/// Both full and shortened (worktree prefix → ⌂) versions are provided.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewLogEntry {
    pub id: u64,
    pub category: LogCategory,
    pub text: String,
    pub short_text: String,
    pub timestamp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_block: Option<AiContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_ai_block: Option<AiContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub housekeeping_block: Option<HousekeepingBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<ToolResultData>,
}

/// Transform raw log records into view-ready entries.
/// ToolResult records are attached to their matching ToolUse entry by tool_id.
/// `worktree_prefix` is used to produce shortened path variants.
pub fn records_to_view_entries(records: &[LogRecord], worktree_prefix: &str) -> Vec<ViewLogEntry> {
    let mut entries = Vec::new();
    let mut tool_use_index: HashMap<String, usize> = HashMap::new();
    let mut id_counter = 0u64;

    let sp = |t: &str| shorten_paths(t, worktree_prefix);

    for record in records {
        match &record.payload {
            SessionEventPayload::Log { category, text } => {
                id_counter += 1;
                // Don't shorten the worktree path log — its purpose is to show
                // the actual directory.
                let short_text = if text.starts_with("Running in worktree") {
                    text.clone()
                } else {
                    sp(text)
                };
                entries.push(ViewLogEntry {
                    id: id_counter,
                    category: category.clone(),
                    text: text.clone(),
                    short_text,
                    timestamp: record.timestamp,
                    ai_block: None,
                    short_ai_block: None,
                    housekeeping_block: None,
                    tool_result: None,
                });
            }
            SessionEventPayload::AiContent { block } => match block {
                AiContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    if let Some(&idx) = tool_use_index.get(tool_use_id) {
                        entries[idx].tool_result = Some(ToolResultData {
                            content: content.clone(),
                            is_error: *is_error,
                        });
                    }
                }
                _ => {
                    id_counter += 1;
                    let text = block.summary();
                    let short_block = block.with_short_paths(worktree_prefix);
                    let short_text = short_block.summary();
                    if let AiContentBlock::ToolUse { tool_id, .. } = block {
                        tool_use_index.insert(tool_id.clone(), entries.len());
                    }
                    entries.push(ViewLogEntry {
                        id: id_counter,
                        category: LogCategory::Ai,
                        text,
                        short_text,
                        timestamp: record.timestamp,
                        ai_block: Some(block.clone()),
                        short_ai_block: Some(short_block),
                        housekeeping_block: None,
                        tool_result: None,
                    });
                }
            },
            SessionEventPayload::Housekeeping { block } => {
                id_counter += 1;
                let text = block.summary();
                entries.push(ViewLogEntry {
                    id: id_counter,
                    category: LogCategory::Git,
                    text: text.clone(),
                    short_text: text,
                    timestamp: record.timestamp,
                    ai_block: None,
                    short_ai_block: None,
                    housekeeping_block: Some(block.clone()),
                    tool_result: None,
                });
            }
            SessionEventPayload::IterationComplete { iteration, tag } => {
                id_counter += 1;
                let tag_str = tag
                    .as_ref()
                    .map(|t| format!(": tagged {}", t))
                    .unwrap_or_default();
                let text = format!("=== Iteration {} complete{} ===", iteration, tag_str);
                entries.push(ViewLogEntry {
                    id: id_counter,
                    category: LogCategory::Script,
                    text: text.clone(),
                    short_text: text,
                    timestamp: record.timestamp,
                    ai_block: None,
                    short_ai_block: None,
                    housekeeping_block: None,
                    tool_result: None,
                });
            }
            SessionEventPayload::RateLimited { message } => {
                id_counter += 1;
                entries.push(ViewLogEntry {
                    id: id_counter,
                    category: LogCategory::Warning,
                    text: message.clone(),
                    short_text: message.clone(),
                    timestamp: record.timestamp,
                    ai_block: None,
                    short_ai_block: None,
                    housekeeping_block: None,
                    tool_result: None,
                });
            }
            SessionEventPayload::ActionRequired { error, .. } => {
                id_counter += 1;
                entries.push(ViewLogEntry {
                    id: id_counter,
                    category: LogCategory::Error,
                    text: error.clone(),
                    short_text: error.clone(),
                    timestamp: record.timestamp,
                    ai_block: None,
                    short_ai_block: None,
                    housekeeping_block: None,
                    tool_result: None,
                });
            }
            _ => {}
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::ToolInvocation;
    use crate::session::state::SessionStep;

    fn make_record(payload: SessionEventPayload) -> LogRecord {
        LogRecord {
            line_no: 0,
            timestamp: 1000,
            payload,
        }
    }

    #[test]
    fn log_entries_are_created() {
        let records = vec![make_record(SessionEventPayload::Log {
            category: LogCategory::Script,
            text: "hello".to_string(),
        })];
        let entries = records_to_view_entries(&records, "");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "hello");
        assert!(matches!(entries[0].category, LogCategory::Script));
    }

    #[test]
    fn worktree_log_not_shortened() {
        let text = "Running in worktree: \"/tmp/wt/proj\" (mode: macos, branch: b)".to_string();
        let records = vec![make_record(SessionEventPayload::Log {
            category: LogCategory::Script,
            text: text.clone(),
        })];
        let entries = records_to_view_entries(&records, "/tmp/wt/proj");
        assert_eq!(entries[0].short_text, text, "worktree log must not be shortened");
    }

    #[test]
    fn tool_result_attaches_to_tool_use() {
        let records = vec![
            make_record(SessionEventPayload::AiContent {
                block: AiContentBlock::ToolUse {
                    tool_id: "t1".to_string(),
                    tool: ToolInvocation::Bash {
                        command: "ls".to_string(),
                        description: None,
                    },
                },
            }),
            make_record(SessionEventPayload::AiContent {
                block: AiContentBlock::ToolResult {
                    tool_use_id: "t1".to_string(),
                    content: "file.txt".to_string(),
                    is_error: false,
                },
            }),
        ];
        let entries = records_to_view_entries(&records, "");
        assert_eq!(
            entries.len(),
            1,
            "ToolResult should not create a separate entry"
        );
        assert!(entries[0].tool_result.is_some());
        let result = entries[0].tool_result.as_ref().unwrap();
        assert_eq!(result.content, "file.txt");
        assert!(!result.is_error);
    }

    #[test]
    fn tool_result_with_no_matching_use_is_dropped() {
        let records = vec![make_record(SessionEventPayload::AiContent {
            block: AiContentBlock::ToolResult {
                tool_use_id: "unknown".to_string(),
                content: "orphan".to_string(),
                is_error: true,
            },
        })];
        let entries = records_to_view_entries(&records, "");
        assert_eq!(
            entries.len(),
            0,
            "Orphan ToolResult should not create an entry"
        );
    }

    #[test]
    fn ai_text_creates_entry_with_ai_category() {
        let records = vec![make_record(SessionEventPayload::AiContent {
            block: AiContentBlock::Text {
                text: "thinking".to_string(),
            },
        })];
        let entries = records_to_view_entries(&records, "");
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].category, LogCategory::Ai));
        assert!(entries[0].ai_block.is_some());
    }

    #[test]
    fn housekeeping_creates_entry_with_git_category() {
        let records = vec![make_record(SessionEventPayload::Housekeeping {
            block: HousekeepingBlock::StepStarted {
                step: SessionStep::Checkout,
                description: "Checking out".to_string(),
            },
        })];
        let entries = records_to_view_entries(&records, "");
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].category, LogCategory::Git));
        assert!(entries[0].housekeeping_block.is_some());
    }

    #[test]
    fn iteration_complete_includes_tag() {
        let records = vec![make_record(SessionEventPayload::IterationComplete {
            iteration: 3,
            tag: Some("1.2.3".to_string()),
        })];
        let entries = records_to_view_entries(&records, "");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].text.contains("Iteration 3"));
        assert!(entries[0].text.contains("tagged 1.2.3"));
    }

    #[test]
    fn iteration_complete_without_tag() {
        let records = vec![make_record(SessionEventPayload::IterationComplete {
            iteration: 1,
            tag: None,
        })];
        let entries = records_to_view_entries(&records, "");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].text.contains("Iteration 1"));
        assert!(!entries[0].text.contains("tagged"));
    }

    #[test]
    fn rate_limited_creates_warning_entry() {
        let records = vec![make_record(SessionEventPayload::RateLimited {
            message: "Slow down".to_string(),
        })];
        let entries = records_to_view_entries(&records, "");
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].category, LogCategory::Warning));
        assert_eq!(entries[0].text, "Slow down");
    }

    #[test]
    fn action_required_creates_error_entry() {
        let records = vec![make_record(SessionEventPayload::ActionRequired {
            error: "dirty worktree".to_string(),
            options: vec![],
        })];
        let entries = records_to_view_entries(&records, "");
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].category, LogCategory::Error));
        assert_eq!(entries[0].text, "dirty worktree");
    }

    #[test]
    fn status_changed_and_finished_are_skipped() {
        let records = vec![
            make_record(SessionEventPayload::StatusChanged {
                status: crate::session::state::SessionStatus::Stopped,
            }),
            make_record(SessionEventPayload::Finished {
                reason: "done".to_string(),
            }),
            make_record(SessionEventPayload::AiSessionIdChanged {
                ai_session_id: Some("s1".to_string()),
            }),
        ];
        let entries = records_to_view_entries(&records, "");
        assert_eq!(
            entries.len(),
            0,
            "Non-visible payloads should produce no entries"
        );
    }

    #[test]
    fn ids_are_sequential() {
        let records = vec![
            make_record(SessionEventPayload::Log {
                category: LogCategory::Script,
                text: "a".to_string(),
            }),
            make_record(SessionEventPayload::Log {
                category: LogCategory::Script,
                text: "b".to_string(),
            }),
            make_record(SessionEventPayload::Log {
                category: LogCategory::Script,
                text: "c".to_string(),
            }),
        ];
        let entries = records_to_view_entries(&records, "");
        assert_eq!(entries[0].id, 1);
        assert_eq!(entries[1].id, 2);
        assert_eq!(entries[2].id, 3);
    }
}
