export interface SessionConfig {
  project_dir: string;
  mode: string;
  prompt_file: string;
  branch_name: string;
  main_branch: string;
  preamble: string;
  tagging_enabled: boolean;
  ai_tool: string;
}

export interface SessionInfo {
  id: { "0": string };
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
  | "RecoveringGit";

export interface LogEntry {
  id: number;
  category: LogCategory;
  text: string;
  timestamp: number;
}

export type LogCategory = "Git" | "Ai" | "Script" | "Warning" | "Error";

export interface SessionState {
  id: string;
  config: SessionConfig;
  status: SessionStatus;
  lastTag: string | null;
  iterationCount: number;
  logs: LogEntry[];
  recoveryRequest: RecoveryRequest | null;
}

export interface ModeInfo {
  name: string;
  prompt_file: string;
  preview: string;
}

export interface AiToolInfo {
  id: string;
  name: string;
}

export interface AppSettings {
  layout: LayoutMode;
  default_ai_tool: string;
  default_main_branch: string;
  default_tagging_enabled: boolean;
  recent_project_dirs: string[];
  recent_preambles: string[];
}

export type LayoutMode = "Sidebar" | "Tabs" | "Split";

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
  | { type: "IterationComplete"; iteration: number; tag: string | null }
  | { type: "Finished"; reason: string }
  | { type: "ActionRequired"; error: string; options: RecoveryOption[] };
