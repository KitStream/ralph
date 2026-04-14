import { invoke } from "@tauri-apps/api/core";
import type {
  ModeInfo,
  AiToolInfo,
  BackendModelConfig,
  SessionInfo,
  AppSettings,
  IterationSummary,
  LogEntry,
  ToolPathInfo,
} from "./types";

export interface CreateSessionRequest {
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

export async function discoverModes(projectDir: string): Promise<ModeInfo[]> {
  return invoke("discover_modes", { projectDir });
}

export async function createSession(
  request: CreateSessionRequest
): Promise<string> {
  return invoke("create_session", { request });
}

export async function startSession(sessionId: string): Promise<void> {
  return invoke("start_session", { sessionId });
}

export async function resumeSession(sessionId: string): Promise<void> {
  return invoke("resume_session", { sessionId });
}

export async function stopSession(sessionId: string): Promise<void> {
  return invoke("stop_session", { sessionId });
}

export async function cancelStopSession(sessionId: string): Promise<void> {
  return invoke("cancel_stop_session", { sessionId });
}

export async function abortSession(sessionId: string): Promise<void> {
  return invoke("abort_session", { sessionId });
}

export async function removeSession(sessionId: string): Promise<void> {
  return invoke("remove_session", { sessionId });
}

export async function listSessions(): Promise<SessionInfo[]> {
  return invoke("list_sessions");
}

export async function getAvailableTools(): Promise<AiToolInfo[]> {
  return invoke("get_available_tools");
}

export async function detectToolPaths(): Promise<ToolPathInfo[]> {
  return invoke("detect_tool_paths");
}

export async function listBackendModels(tool: string): Promise<BackendModelConfig> {
  return invoke("list_backend_models", { tool });
}

export async function getSettings(): Promise<AppSettings> {
  return invoke("get_settings");
}

export async function updateSettings(settings: AppSettings): Promise<void> {
  return invoke("update_settings", { settings });
}

export async function listLogIterations(sessionId: string): Promise<IterationSummary[]> {
  return invoke("list_log_iterations", { sessionId });
}

export async function readLogIterationView(sessionId: string, iteration: number): Promise<LogEntry[]> {
  return invoke("read_log_iteration_view", { sessionId, iteration });
}

export async function sendRecoveryAction(
  sessionId: string,
  action: string
): Promise<void> {
  return invoke("send_recovery_action", { sessionId, action });
}
