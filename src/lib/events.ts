import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { SessionEvent } from "./types";

export function listenToSessionEvents(
  callback: (event: SessionEvent) => void
): Promise<UnlistenFn> {
  return listen<SessionEvent>("session-event", (e) => {
    callback(e.payload);
  });
}
