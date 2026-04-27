import { useState } from "react";
import { LogView } from "./LogView";
import { StatusBar } from "./StatusBar";
import { ControlBar } from "./ControlBar";
import { useSession } from "../hooks/useSession";
import { useSessions } from "../hooks/useSessions";

interface SessionPanelProps {
  sessionId: string | null;
}

export function SessionPanel({ sessionId }: SessionPanelProps) {
  const { session, start, resume, stop, cancelStop, abort, remove, toggleFoldIteration } = useSession(sessionId);
  const { state } = useSessions();
  const [shortenPaths, setShortenPaths] = useState(true);
  // Tool output is collapsed by default; the runs are noisy and most of the
  // time the user only wants the AI's text and the housekeeping summary.
  const [showToolOutput, setShowToolOutput] = useState(false);

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
      <LogView
        iterations={session.iterations}
        iterationLogs={session.iterationLogs}
        foldedIterations={session.foldedIterations}
        onToggleFold={toggleFoldIteration}
        shortenPaths={shortenPaths}
        showToolOutput={showToolOutput}
        toolOutputPreviewLines={state.settings.tool_output_preview_lines}
        rateLimitMessage={session.rateLimitMessage}
      />
      <StatusBar
        status={session.status}
        iterationCount={session.iterationCount}
        lastTag={session.lastTag}
        mode={session.config.mode}
        aiTool={session.config.ai_tool}
        shortenPaths={shortenPaths}
        onToggleShortenPaths={() => setShortenPaths((v) => !v)}
        showToolOutput={showToolOutput}
        onToggleShowToolOutput={() => setShowToolOutput((v) => !v)}
      />
    </div>
  );
}
