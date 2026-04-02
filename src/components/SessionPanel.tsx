import { LogView } from "./LogView";
import { StatusBar } from "./StatusBar";
import { ControlBar } from "./ControlBar";
import { useSession } from "../hooks/useSession";

interface SessionPanelProps {
  sessionId: string | null;
}

export function SessionPanel({ sessionId }: SessionPanelProps) {
  const { session, start, stop, abort, remove } = useSession(sessionId);

  if (!session) {
    return (
      <div
        style={{
          flex: 1,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: "#6b7280",
          backgroundColor: "#0d1117",
        }}
      >
        No session selected. Create one to get started.
      </div>
    );
  }

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column" }}>
      <ControlBar
        status={session.status}
        onStart={start}
        onStop={stop}
        onAbort={abort}
        onRemove={remove}
      />
      <LogView logs={session.logs} />
      <StatusBar
        status={session.status}
        iterationCount={session.iterationCount}
        lastTag={session.lastTag}
        mode={session.config.mode}
        aiTool={session.config.ai_tool}
      />
    </div>
  );
}
