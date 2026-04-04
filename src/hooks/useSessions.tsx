import {
  createContext,
  useContext,
  useEffect,
  useReducer,
  useCallback,
  type ReactNode,
} from "react";
import { listenToSessionEvents } from "../lib/events";
import * as commands from "../lib/commands";
import type {
  SessionState,
  SessionEvent,
  SessionEventPayload,
  LogEntry,
  ToolResultData,
  SessionStatus,
  AppSettings,
  AiContentBlock,
  HousekeepingBlock,
  IterationSummary,
} from "../lib/types";

interface AppState {
  sessions: Map<string, SessionState>;
  activeSessionId: string | null;
  settings: AppSettings;
}

type Action =
  | { type: "SET_SESSIONS"; sessions: Map<string, SessionState> }
  | { type: "ADD_SESSION"; session: SessionState }
  | { type: "REMOVE_SESSION"; id: string }
  | { type: "SET_ACTIVE"; id: string | null }
  | { type: "SESSION_EVENT"; event: SessionEvent }
  | { type: "SET_SETTINGS"; settings: AppSettings }
  | { type: "MARK_STOPPING"; id: string }
  | { type: "CANCEL_STOPPING"; id: string }
  | { type: "LOAD_ITERATIONS"; sessionId: string; iterations: IterationSummary[] }
  | { type: "SET_ITERATION_LOGS"; sessionId: string; iteration: number; entries: LogEntry[] }
  | { type: "TOGGLE_FOLD_ITERATION"; sessionId: string; iteration: number };

let logIdCounter = 0;

function makeEmptySessionState(id: string, config: SessionState["config"], status: SessionStatus, iterationCount: number, lastTag: string | null): SessionState {
  return {
    id,
    config,
    status,
    lastTag,
    iterationCount,
    iterations: [],
    iterationLogs: new Map(),
    foldedIterations: new Set(),
    currentIteration: iterationCount > 0 ? iterationCount + 1 : 1,
    recoveryRequest: null,
    rateLimitMessage: null,
  };
}

function reducer(state: AppState, action: Action): AppState {
  switch (action.type) {
    case "SET_SESSIONS":
      return { ...state, sessions: action.sessions };

    case "ADD_SESSION": {
      const sessions = new Map(state.sessions);
      sessions.set(action.session.id, action.session);
      return {
        ...state,
        sessions,
        activeSessionId: action.session.id,
      };
    }

    case "REMOVE_SESSION": {
      const sessions = new Map(state.sessions);
      sessions.delete(action.id);
      const activeSessionId =
        state.activeSessionId === action.id
          ? (sessions.keys().next().value ?? null)
          : state.activeSessionId;
      return { ...state, sessions, activeSessionId };
    }

    case "SET_ACTIVE":
      return { ...state, activeSessionId: action.id };

    case "SESSION_EVENT": {
      const { session_id, payload } = action.event;
      const session = state.sessions.get(session_id);
      if (!session) return state;

      const updated = applyEvent(session, payload);
      const sessions = new Map(state.sessions);
      sessions.set(session_id, updated);
      return { ...state, sessions };
    }

    case "SET_SETTINGS":
      return { ...state, settings: action.settings };

    case "MARK_STOPPING": {
      const session = state.sessions.get(action.id);
      if (!session) return state;
      let step: import("../lib/types").SessionStep = "Idle";
      let iteration = session.iterationCount || 0;
      if (typeof session.status === "object" && "Running" in session.status) {
        step = session.status.Running.step;
        iteration = session.status.Running.iteration;
      }
      const updated: SessionState = {
        ...session,
        status: { Stopping: { step, iteration } },
      };
      const sessions = new Map(state.sessions);
      sessions.set(action.id, updated);
      return { ...state, sessions };
    }

    case "CANCEL_STOPPING": {
      const session = state.sessions.get(action.id);
      if (!session) return state;
      let step: import("../lib/types").SessionStep = "Idle";
      let iteration = session.iterationCount || 0;
      if (typeof session.status === "object" && "Stopping" in session.status) {
        step = session.status.Stopping.step;
        iteration = session.status.Stopping.iteration;
      }
      const updated: SessionState = {
        ...session,
        status: { Running: { step, iteration } },
      };
      const sessions = new Map(state.sessions);
      sessions.set(action.id, updated);
      return { ...state, sessions };
    }

    case "LOAD_ITERATIONS": {
      const session = state.sessions.get(action.sessionId);
      if (!session) return state;
      const nextIteration = session.iterationCount > 0 ? session.iterationCount + 1 : 1;
      const folded = new Set<number>();
      for (const s of action.iterations) {
        if (s.iteration <= nextIteration - 4) {
          folded.add(s.iteration);
        }
      }
      const updated: SessionState = {
        ...session,
        iterations: action.iterations,
        foldedIterations: folded,
        currentIteration: nextIteration,
      };
      const sessions = new Map(state.sessions);
      sessions.set(action.sessionId, updated);
      return { ...state, sessions };
    }

    case "SET_ITERATION_LOGS": {
      const session = state.sessions.get(action.sessionId);
      if (!session) return state;
      const iterationLogs = new Map(session.iterationLogs);
      iterationLogs.set(action.iteration, action.entries);
      const updated: SessionState = { ...session, iterationLogs };
      const sessions = new Map(state.sessions);
      sessions.set(action.sessionId, updated);
      return { ...state, sessions };
    }

    case "TOGGLE_FOLD_ITERATION": {
      const session = state.sessions.get(action.sessionId);
      if (!session) return state;
      const foldedIterations = new Set(session.foldedIterations);
      const iterationLogs = new Map(session.iterationLogs);
      if (foldedIterations.has(action.iteration)) {
        foldedIterations.delete(action.iteration);
        // Entries will be loaded asynchronously by the component
      } else {
        foldedIterations.add(action.iteration);
        // Free memory
        iterationLogs.delete(action.iteration);
      }
      const updated: SessionState = { ...session, foldedIterations, iterationLogs };
      const sessions = new Map(state.sessions);
      sessions.set(action.sessionId, updated);
      return { ...state, sessions };
    }

    default:
      return state;
  }
}

function appendLogEntry(session: SessionState, entry: LogEntry): SessionState {
  const iter = session.currentIteration;
  const iterationLogs = new Map(session.iterationLogs);
  const existing = iterationLogs.get(iter) ?? [];
  iterationLogs.set(iter, [...existing, entry]);

  // Update iteration summary
  const iterations = [...session.iterations];
  const idx = iterations.findIndex((s) => s.iteration === iter);
  if (idx >= 0) {
    iterations[idx] = { ...iterations[idx], entry_count: iterations[idx].entry_count + 1 };
  } else {
    iterations.push({ iteration: iter, entry_count: 1 });
  }

  return { ...session, iterationLogs, iterations };
}

function attachToolResult(session: SessionState, toolUseId: string, result: ToolResultData): SessionState {
  // Search backwards through iterations to find the matching ToolUse entry
  const iterationLogs = new Map(session.iterationLogs);
  for (const [iter, entries] of iterationLogs) {
    for (let i = entries.length - 1; i >= 0; i--) {
      const entry = entries[i];
      if (entry.aiBlock?.kind === "ToolUse" && entry.aiBlock.tool_id === toolUseId) {
        const updated = [...entries];
        updated[i] = { ...entry, toolResult: result };
        iterationLogs.set(iter, updated);
        return { ...session, iterationLogs };
      }
    }
  }
  // Fallback: if no matching ToolUse found, append as a standalone entry
  const fallbackEntry: LogEntry = {
    id: ++logIdCounter,
    category: "Ai",
    text: result.content.slice(0, 200),
    timestamp: Date.now(),
    aiBlock: { kind: "ToolResult", tool_use_id: toolUseId, content: result.content, is_error: result.is_error },
  };
  return appendLogEntry(session, fallbackEntry);
}

function applyEvent(
  session: SessionState,
  payload: SessionEventPayload
): SessionState {
  switch (payload.type) {
    case "StatusChanged": {
      // Don't let the runner's Stopped/Failed overwrite an Aborted status
      // (the manager already guards this server-side; mirror it here).
      const dominated = payload.status === "Stopped" || (typeof payload.status === "object" && "Failed" in payload.status);
      const currentlyAborted = typeof session.status === "object" && "Aborted" in session.status;
      if (dominated && currentlyAborted) return session;
      return { ...session, status: payload.status, recoveryRequest: null, rateLimitMessage: null };
    }

    case "Log": {
      const entry: LogEntry = {
        id: ++logIdCounter,
        category: payload.category,
        text: payload.text,
        timestamp: Date.now(),
      };
      return appendLogEntry(session, entry);
    }

    case "AiContent": {
      // ToolResult: attach to matching ToolUse entry instead of appending
      if (payload.block.kind === "ToolResult") {
        return attachToolResult(session, payload.block.tool_use_id, {
          content: payload.block.content,
          is_error: payload.block.is_error,
        });
      }
      const entry: LogEntry = {
        id: ++logIdCounter,
        category: "Ai",
        text: summarizeAiBlock(payload.block),
        timestamp: Date.now(),
        aiBlock: payload.block,
      };
      return appendLogEntry(session, entry);
    }

    case "Housekeeping": {
      const entry: LogEntry = {
        id: ++logIdCounter,
        category: "Git",
        text: summarizeHousekeepingBlock(payload.block),
        timestamp: Date.now(),
        housekeepingBlock: payload.block,
      };
      return appendLogEntry(session, entry);
    }

    case "RateLimited":
      return { ...session, rateLimitMessage: payload.message };

    case "IterationComplete": {
      const completedIteration = payload.iteration;
      const nextIteration = completedIteration + 1;
      // Auto-fold iterations more than 3 behind the next one
      const foldedIterations = new Set(session.foldedIterations);
      const iterationLogs = new Map(session.iterationLogs);
      for (const s of session.iterations) {
        if (s.iteration <= nextIteration - 4 && !foldedIterations.has(s.iteration)) {
          foldedIterations.add(s.iteration);
          iterationLogs.delete(s.iteration);
        }
      }
      return {
        ...session,
        iterationCount: completedIteration,
        currentIteration: nextIteration,
        lastTag: payload.tag ?? session.lastTag,
        foldedIterations,
        iterationLogs,
      };
    }

    case "Finished": {
      // Don't overwrite Aborted with Stopped — preserve resume capability
      const aborted = typeof session.status === "object" && "Aborted" in session.status;
      if (aborted) return { ...session, recoveryRequest: null };
      return { ...session, status: "Stopped" as SessionStatus, recoveryRequest: null };
    }

    case "ActionRequired":
      return {
        ...session,
        recoveryRequest: {
          sessionId: session.id,
          error: payload.error,
          options: payload.options,
        },
      };

    default:
      return session;
  }
}

function summarizeAiBlock(block: AiContentBlock): string {
  switch (block.kind) {
    case "Text":
      return block.text;
    case "ToolUse": {
      const t = block.tool;
      switch (t.tool) {
        case "Read": return `Read ${t.file_path}`;
        case "Edit": return `Edit ${t.file_path}`;
        case "Write": return `Write ${t.file_path}`;
        case "Bash": return `$ ${t.command}`;
        case "Glob": return `Glob ${t.pattern}`;
        case "Grep": return `Grep ${t.pattern}`;
        case "Other": return `${t.name}`;
      }
      break;
    }
    case "ToolResult":
      return block.content.slice(0, 200);
  }
  return "";
}

function summarizeHousekeepingBlock(block: HousekeepingBlock): string {
  switch (block.kind) {
    case "StepStarted": return `▸ ${block.description}`;
    case "StepCompleted": return `✓ ${block.summary}`;
    case "GitCommand": return block.output;
    case "DiffStat": return block.stat;
    case "Recovery": return `${block.action}: ${block.detail}`;
  }
}

interface SessionsContextType {
  state: AppState;
  dispatch: React.Dispatch<Action>;
  createSession: (req: commands.CreateSessionRequest) => Promise<string>;
  startSession: (id: string) => Promise<void>;
  resumeSession: (id: string) => Promise<void>;
  stopSession: (id: string) => Promise<void>;
  cancelStopSession: (id: string) => Promise<void>;
  abortSession: (id: string) => Promise<void>;
  removeSession: (id: string) => Promise<void>;
  setActiveSession: (id: string | null) => void;
  updateSettings: (settings: AppSettings) => Promise<void>;
  loadIterationLogs: (sessionId: string, iteration: number) => Promise<void>;
  toggleFoldIteration: (sessionId: string, iteration: number) => void;
}

const SessionsContext = createContext<SessionsContextType | null>(null);

function logRecordsToEntries(records: import("../lib/types").LogRecord[]): LogEntry[] {
  const entries: LogEntry[] = [];
  // Map tool_id -> index in entries for attaching results
  const toolUseIndex = new Map<string, number>();

  for (const record of records) {
    const p = record.payload;
    let entry: LogEntry | null = null;
    switch (p.type) {
      case "Log":
        entry = { id: ++logIdCounter, category: p.category, text: p.text, timestamp: record.timestamp };
        break;
      case "AiContent":
        if (p.block.kind === "ToolResult") {
          // Attach to matching ToolUse
          const idx = toolUseIndex.get(p.block.tool_use_id);
          if (idx !== undefined) {
            entries[idx] = { ...entries[idx], toolResult: { content: p.block.content, is_error: p.block.is_error } };
          }
          continue; // Don't add as separate entry
        }
        entry = { id: ++logIdCounter, category: "Ai", text: summarizeAiBlock(p.block), timestamp: record.timestamp, aiBlock: p.block };
        if (p.block.kind === "ToolUse") {
          toolUseIndex.set(p.block.tool_id, entries.length); // will be pushed next
        }
        break;
      case "Housekeeping":
        entry = { id: ++logIdCounter, category: "Git", text: summarizeHousekeepingBlock(p.block), timestamp: record.timestamp, housekeepingBlock: p.block };
        break;
      case "IterationComplete":
        entry = { id: ++logIdCounter, category: "Script", text: `=== Iteration ${p.iteration} complete${p.tag ? `: tagged ${p.tag}` : ""} ===`, timestamp: record.timestamp };
        break;
      case "RateLimited":
        entry = { id: ++logIdCounter, category: "Warning", text: p.message, timestamp: record.timestamp };
        break;
      case "ActionRequired":
        entry = { id: ++logIdCounter, category: "Error", text: p.error, timestamp: record.timestamp };
        break;
    }
    if (entry) entries.push(entry);
  }
  return entries;
}

export function SessionsProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, {
    sessions: new Map(),
    activeSessionId: null,
    settings: {
      layout: "Sidebar",
      theme: "Dark",
      default_ai_tool: "claude",
      default_main_branch: "main",
      default_tagging_enabled: true,
      recent_project_dirs: [],
      recent_preambles: [],
      tool_output_preview_lines: 2,
    },
  });

  // Load settings and persisted sessions on mount
  useEffect(() => {
    commands.getSettings().then((settings) => {
      dispatch({ type: "SET_SETTINGS", settings });
    });
    commands.listSessions().then(async (infos) => {
      const sessions = new Map<string, SessionState>();
      for (const info of infos) {
        const id = typeof info.id === "string" ? info.id : String(info.id);
        sessions.set(id, makeEmptySessionState(
          id,
          info.config,
          info.status,
          info.iteration_count,
          info.last_tag,
        ));
      }
      if (sessions.size > 0) {
        dispatch({ type: "SET_SESSIONS", sessions });
      }

      // Load iteration summaries for each session
      for (const [id] of sessions) {
        commands.listLogIterations(id).then(async (iterations) => {
          dispatch({ type: "LOAD_ITERATIONS", sessionId: id, iterations });

          // Load the last 3 iterations' logs
          const info = infos.find((i) => (typeof i.id === "string" ? i.id : String(i.id)) === id);
          const currentIter = info?.iteration_count ?? 0;
          for (const s of iterations) {
            if (s.iteration > currentIter - 3) {
              const records = await commands.readLogIteration(id, s.iteration);
              const entries = logRecordsToEntries(records);
              dispatch({ type: "SET_ITERATION_LOGS", sessionId: id, iteration: s.iteration, entries });
            }
          }
        });
      }
    });
  }, []);

  // Subscribe to session events
  useEffect(() => {
    const unlisten = listenToSessionEvents((event) => {
      dispatch({ type: "SESSION_EVENT", event });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const persistSessionDefaults = useCallback(
    async (req: commands.CreateSessionRequest) => {
      const current = await commands.getSettings();
      const dirs = [req.project_dir, ...current.recent_project_dirs.filter((d) => d !== req.project_dir)].slice(0, 10);
      const preambles = req.preamble
        ? [req.preamble, ...current.recent_preambles.filter((p) => p !== req.preamble)].slice(0, 100)
        : current.recent_preambles;
      const updated = {
        ...current,
        recent_project_dirs: dirs,
        default_ai_tool: req.ai_tool,
        recent_preambles: preambles,
      };
      await commands.updateSettings(updated);
      dispatch({ type: "SET_SETTINGS", settings: updated });
    },
    []
  );

  const createSessionFn = useCallback(
    async (req: commands.CreateSessionRequest) => {
      const id = await commands.createSession(req);
      const session = makeEmptySessionState(
        id,
        {
          project_dir: req.project_dir,
          mode: req.mode,
          prompt_file: req.prompt_file,
          branch_name: req.branch_name,
          main_branch: req.main_branch,
          preamble: req.preamble,
          tagging_enabled: req.tagging_enabled,
          ai_tool: req.ai_tool,
          model: req.model,
        },
        "Created",
        0,
        null,
      );
      dispatch({ type: "ADD_SESSION", session });
      persistSessionDefaults(req);
      return id;
    },
    [persistSessionDefaults]
  );

  const startSessionFn = useCallback(async (id: string) => {
    try {
      await commands.startSession(id);
    } catch (e) {
      console.error("startSession failed:", e);
    }
  }, []);

  const resumeSessionFn = useCallback(async (id: string) => {
    try {
      await commands.resumeSession(id);
    } catch (e) {
      console.error("resumeSession failed:", e);
    }
  }, []);

  const stopSessionFn = useCallback(async (id: string) => {
    dispatch({ type: "MARK_STOPPING", id });
    await commands.stopSession(id);
  }, []);

  const cancelStopSessionFn = useCallback(async (id: string) => {
    dispatch({ type: "CANCEL_STOPPING", id });
    await commands.cancelStopSession(id);
  }, []);

  const abortSessionFn = useCallback(async (id: string) => {
    await commands.abortSession(id);
  }, []);

  const removeSessionFn = useCallback(async (id: string) => {
    await commands.removeSession(id);
    dispatch({ type: "REMOVE_SESSION", id });
  }, []);

  const setActiveSessionFn = useCallback((id: string | null) => {
    dispatch({ type: "SET_ACTIVE", id });
  }, []);

  const updateSettingsFn = useCallback(async (settings: AppSettings) => {
    await commands.updateSettings(settings);
    dispatch({ type: "SET_SETTINGS", settings });
  }, []);

  const loadIterationLogsFn = useCallback(async (sessionId: string, iteration: number) => {
    const records = await commands.readLogIteration(sessionId, iteration);
    const entries = logRecordsToEntries(records);
    dispatch({ type: "SET_ITERATION_LOGS", sessionId, iteration, entries });
  }, []);

  const toggleFoldIterationFn = useCallback((sessionId: string, iteration: number) => {
    dispatch({ type: "TOGGLE_FOLD_ITERATION", sessionId, iteration });
  }, []);

  return (
    <SessionsContext.Provider
      value={{
        state,
        dispatch,
        createSession: createSessionFn,
        startSession: startSessionFn,
        resumeSession: resumeSessionFn,
        stopSession: stopSessionFn,
        cancelStopSession: cancelStopSessionFn,
        abortSession: abortSessionFn,
        removeSession: removeSessionFn,
        setActiveSession: setActiveSessionFn,
        updateSettings: updateSettingsFn,
        loadIterationLogs: loadIterationLogsFn,
        toggleFoldIteration: toggleFoldIterationFn,
      }}
    >
      {children}
    </SessionsContext.Provider>
  );
}

export function useSessions() {
  const ctx = useContext(SessionsContext);
  if (!ctx) throw new Error("useSessions must be used within SessionsProvider");
  return ctx;
}
