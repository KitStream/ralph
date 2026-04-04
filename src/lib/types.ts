export interface SessionConfig {
  project_dir: string;
  mode: string;
  prompt_file: string;
  branch_name: string;
  main_branch: string;
  preamble: string;
  tagging_enabled: boolean;
  ai_tool: string;
  model: string | null;
}

export interface SessionInfo {
  id: string;
  config: SessionConfig;
  status: SessionStatus;
  last_tag: string | null;
  iteration_count: number;
}

export type SessionStatus =
  | "Created"
  | { Running: { step: SessionStep; iteration: number } }
  | { Stopping: { step: SessionStep; iteration: number } }
  | "Stopped"
  | { Aborted: { ai_session_id: string | null; step: SessionStep | null; iteration: number | null } }
  | { Failed: { error: string } };

export type SessionStep =
  | "Idle"
  | "Checkout"
  | "RebasePreAi"
  | "RunningAi"
  | "PushBranch"
  | "RebasePostAi"
  | "PushToMain"
  | "Tagging"
  | "RecoveringGit"
  | "Paused";

export type ToolInvocation =
  | { tool: "Read"; file_path: string }
  | { tool: "Edit"; file_path: string; old_string: string; new_string: string }
  | { tool: "Write"; file_path: string; content: string }
  | { tool: "Bash"; command: string; description: string | null }
  | { tool: "Glob"; pattern: string; path: string | null }
  | { tool: "Grep"; pattern: string; path: string | null; include: string | null }
  | { tool: "Other"; name: string; input: Record<string, unknown> };

export type AiContentBlock =
  | { kind: "Text"; text: string }
  | { kind: "ToolUse"; tool_id: string; tool: ToolInvocation }
  | { kind: "ToolResult"; tool_use_id: string; content: string; is_error: boolean };

export type HousekeepingBlock =
  | { kind: "StepStarted"; step: SessionStep; description: string }
  | { kind: "StepCompleted"; step: SessionStep; summary: string }
  | { kind: "GitCommand"; command: string; output: string; success: boolean }
  | { kind: "DiffStat"; stat: string }
  | { kind: "Recovery"; action: string; detail: string };

export interface ToolResultData {
  content: string;
  is_error: boolean;
}

export interface LogEntry {
  id: number;
  category: LogCategory;
  text: string;
  shortText: string;
  timestamp: number;
  aiBlock?: AiContentBlock;
  shortAiBlock?: AiContentBlock;
  housekeepingBlock?: HousekeepingBlock;
  toolResult?: ToolResultData;
}

export type LogCategory = "Git" | "Ai" | "Script" | "Warning" | "Error" | "Prompt";

export interface IterationSummary {
  iteration: number;
  entry_count: number;
}

export interface SessionState {
  id: string;
  config: SessionConfig;
  status: SessionStatus;
  lastTag: string | null;
  iterationCount: number;
  iterations: IterationSummary[];
  iterationLogs: Map<number, LogEntry[]>;
  foldedIterations: Set<number>;
  currentIteration: number;
  recoveryRequest: RecoveryRequest | null;
  rateLimitMessage: string | null;
}

export interface ModeInfo {
  name: string;
  prompt_file: string;
  preview: string;
}

export interface AiToolInfo {
  id: string;
  name: string;
  available: boolean;
}

export interface ModelInfo {
  id: string;
  label: string;
  is_default: boolean;
}

export interface BackendModelConfig {
  models: ModelInfo[];
  supports_freeform: boolean;
  current_model: string | null;
}

export interface AppSettings {
  layout: LayoutMode;
  theme: ThemeMode;
  default_ai_tool: string;
  default_main_branch: string;
  default_tagging_enabled: boolean;
  recent_project_dirs: string[];
  recent_preambles: string[];
  default_mode: string;
  default_preamble: string;
  tool_output_preview_lines: number;
}

export type LayoutMode = "Sidebar" | "Tabs" | "Split";
export type ThemeMode = "Dark" | "Light";

export interface SessionEvent {
  session_id: string;
  payload: SessionEventPayload;
}

export interface RecoveryOption {
  id: string;
  label: string;
  description: string;
}

export interface RecoveryRequest {
  sessionId: string;
  error: string;
  options: RecoveryOption[];
}

export type SessionEventPayload =
  | { type: "StatusChanged"; status: SessionStatus }
  | { type: "Log"; category: LogCategory; text: string }
  | { type: "AiContent"; block: AiContentBlock }
  | { type: "Housekeeping"; block: HousekeepingBlock }
  | { type: "IterationComplete"; iteration: number; tag: string | null }
  | { type: "Finished"; reason: string }
  | { type: "AiSessionIdChanged"; ai_session_id: string | null }
  | { type: "RateLimited"; message: string }
  | { type: "ActionRequired"; error: string; options: RecoveryOption[] };
