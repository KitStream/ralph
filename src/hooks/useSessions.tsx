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
import { worktreePrefix, shortenPaths, shortenAiBlock } from "../lib/paths";

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
      } else {
        foldedIterations.add(action.iteration);
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
  const truncated = result.content.slice(0, 200);
  const fallbackEntry: LogEntry = {
    id: ++logIdCounter,
    category: "Ai",
    text: truncated,
    shortText: truncated,
    timestamp: Date.now(),
    aiBlock: { kind: "ToolResult", tool_use_id: toolUseId, content: result.content, is_error: result.is_error },
  };
  return appendLogEntry(session, fallbackEntry);
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


function applyEvent(
  session: SessionState,
  payload: SessionEventPayload
): SessionState {
  const wp = worktreePrefix(session.config.project_dir, session.config.branch_name);

  switch (payload.type) {
    case "StatusChanged":
      return { ...session, status: payload.status, recoveryRequest: null, rateLimitMessage: null };

    case "Log": {
      const text = payload.text;
      const entry: LogEntry = {
        id: ++logIdCounter,
        category: payload.category,
        text,
        shortText: text.startsWith("Running in worktree") ? text : shortenPaths(text, wp),
        timestamp: Date.now(),
      };
      return appendLogEntry(session, entry);
    }

    case "AiContent": {
      if (payload.block.kind === "ToolResult") {
        return attachToolResult(session, payload.block.tool_use_id, {
          content: payload.block.content,
          is_error: payload.block.is_error,
        });
      }
      const text = summarizeAiBlock(payload.block);
      const shortBlock = shortenAiBlock(payload.block, wp);
      const entry: LogEntry = {
        id: ++logIdCounter,
        category: "Ai",
        text,
        shortText: summarizeAiBlock(shortBlock),
        timestamp: Date.now(),
        aiBlock: payload.block,
        shortAiBlock: shortBlock,
      };
      return appendLogEntry(session, entry);
    }

    case "Housekeeping": {
      const text = summarizeHousekeepingBlock(payload.block);
      const entry: LogEntry = {
        id: ++logIdCounter,
        category: "Git",
        text,
        shortText: text,
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
      default_mode: "",
      default_preamble: "",
      tool_output_preview_lines: 2,
    },
  });

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

      for (const [id] of sessions) {
        commands.listLogIterations(id).then(async (iterations) => {
          dispatch({ type: "LOAD_ITERATIONS", sessionId: id, iterations });

          const info = infos.find((i) => (typeof i.id === "string" ? i.id : String(i.id)) === id);
          const currentIter = info?.iteration_count ?? 0;
          for (const s of iterations) {
            if (s.iteration > currentIter - 3) {
              const entries = await commands.readLogIterationView(id, s.iteration);
              dispatch({ type: "SET_ITERATION_LOGS", sessionId: id, iteration: s.iteration, entries });
            }
          }
        });
      }
    });
  }, []);

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
        default_mode: req.mode,
        default_preamble: req.preamble,
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
    await commands.stopSession(id);
  }, []);

  const cancelStopSessionFn = useCallback(async (id: string) => {
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
    const entries = await commands.readLogIterationView(sessionId, iteration);
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
