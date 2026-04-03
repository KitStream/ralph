import { useState } from "react";
import { LogView } from "./LogView";
import { StatusBar } from "./StatusBar";
import { ControlBar } from "./ControlBar";
import { useSession } from "../hooks/useSession";

interface SessionPanelProps {
  sessionId: string | null;
}

export function SessionPanel({ sessionId }: SessionPanelProps) {
  const { session, start, resume, stop, cancelStop, abort, remove } = useSession(sessionId);
  const [shortenPaths, setShortenPaths] = useState(true);

  if (!session) {
    return (
      <div
        style={{
          flex: 1,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: "var(--text-muted)",
          backgroundColor: "var(--bg-primary)",
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
        onResume={resume}
        onStop={stop}
        onCancelStop={cancelStop}
        onAbort={abort}
        onClose={remove}
      />
      <LogView logs={session.logs} projectDir={shortenPaths ? session.config.project_dir : undefined} branchName={shortenPaths ? session.config.branch_name : undefined} rateLimitMessage={session.rateLimitMessage} />
      <StatusBar
        status={session.status}
        iterationCount={session.iterationCount}
        lastTag={session.lastTag}
        mode={session.config.mode}
        aiTool={session.config.ai_tool}
        shortenPaths={shortenPaths}
        onToggleShortenPaths={() => setShortenPaths((v) => !v)}
      />
    </div>
  );
}
