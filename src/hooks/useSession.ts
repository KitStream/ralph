import { useCallback } from "react";
import { useSessions } from "./useSessions";
import type { SessionState } from "../lib/types";

export function useSession(id: string | null) {
  const { state, startSession, resumeSession, stopSession, cancelStopSession, abortSession, removeSession, loadIterationLogs, toggleFoldIteration } =
    useSessions();

  const session: SessionState | null = id
    ? state.sessions.get(id) ?? null
    : null;

  const toggleFold = useCallback((iteration: number) => {
    if (!id) return;
    const s = state.sessions.get(id);
    if (!s) return;
    toggleFoldIteration(id, iteration);
    // If unfolding and no entries loaded, load them
    if (s.foldedIterations.has(iteration)) {
      loadIterationLogs(id, iteration);
    }
  }, [id, state.sessions, toggleFoldIteration, loadIterationLogs]);

  return {
    session,
    start: () => id && startSession(id),
    resume: () => id && resumeSession(id),
    stop: () => id && stopSession(id),
    cancelStop: () => id && cancelStopSession(id),
    abort: () => id && abortSession(id),
    remove: () => id && removeSession(id),
    toggleFoldIteration: toggleFold,
  };
}
