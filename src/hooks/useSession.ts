import { useSessions } from "./useSessions";
import type { SessionState } from "../lib/types";

export function useSession(id: string | null) {
  const { state, startSession, resumeSession, stopSession, abortSession, removeSession } =
    useSessions();

  const session: SessionState | null = id
    ? state.sessions.get(id) ?? null
    : null;

  return {
    session,
    start: () => id && startSession(id),
    resume: () => id && resumeSession(id),
    stop: () => id && stopSession(id),
    abort: () => id && abortSession(id),
    remove: () => id && removeSession(id),
  };
}
