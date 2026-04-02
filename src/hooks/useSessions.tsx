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
  SessionStatus,
  AppSettings,
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
  | { type: "MARK_STOPPING"; id: string };

let logIdCounter = 0;

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
        activeSessionId: state.activeSessionId ?? action.session.id,
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
      // Extract current step/iteration from Running status, or use defaults
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

    default:
      return state;
  }
}

function applyEvent(
  session: SessionState,
  payload: SessionEventPayload
): SessionState {
  switch (payload.type) {
    case "StatusChanged":
      return { ...session, status: payload.status, recoveryRequest: null };

    case "Log": {
      const entry: LogEntry = {
        id: ++logIdCounter,
        category: payload.category,
        text: payload.text,
        timestamp: Date.now(),
      };
      return { ...session, logs: [...session.logs, entry] };
    }

    case "IterationComplete":
      return {
        ...session,
        iterationCount: payload.iteration,
        lastTag: payload.tag ?? session.lastTag,
      };

    case "Finished":
      return { ...session, status: "Stopped" as SessionStatus, recoveryRequest: null };

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
  abortSession: (id: string) => Promise<void>;
  removeSession: (id: string) => Promise<void>;
  setActiveSession: (id: string | null) => void;
  updateSettings: (settings: AppSettings) => Promise<void>;
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
    },
  });

  // Load settings and persisted sessions on mount
  useEffect(() => {
    commands.getSettings().then((settings) => {
      dispatch({ type: "SET_SETTINGS", settings });
    });
    commands.listSessions().then((infos) => {
      const sessions = new Map<string, SessionState>();
      for (const info of infos) {
        const id = typeof info.id === "string" ? info.id : String(info.id);
        sessions.set(id, {
          id,
          config: info.config,
          status: info.status,
          lastTag: info.last_tag,
          iterationCount: info.iteration_count,
          logs: [],
          recoveryRequest: null,
        });
      }
      if (sessions.size > 0) {
        dispatch({ type: "SET_SESSIONS", sessions });
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
      const session: SessionState = {
        id,
        config: {
          project_dir: req.project_dir,
          mode: req.mode,
          prompt_file: req.prompt_file,
          branch_name: req.branch_name,
          main_branch: req.main_branch,
          preamble: req.preamble,
          tagging_enabled: req.tagging_enabled,
          ai_tool: req.ai_tool,
        },
        status: "Created",
        lastTag: null,
        iterationCount: 0,
        logs: [],
        recoveryRequest: null,
      };
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

  return (
    <SessionsContext.Provider
      value={{
        state,
        dispatch,
        createSession: createSessionFn,
        startSession: startSessionFn,
        resumeSession: resumeSessionFn,
        stopSession: stopSessionFn,
        abortSession: abortSessionFn,
        removeSession: removeSessionFn,
        setActiveSession: setActiveSessionFn,
        updateSettings: updateSettingsFn,
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
