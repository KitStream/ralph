import type { SessionStatus } from "../lib/types";

interface ControlBarProps {
  status: SessionStatus;
  onStart: () => void;
  onStop: () => void;
  onAbort: () => void;
  onRemove: () => void;
}

function isRunning(status: SessionStatus): boolean {
  return typeof status === "object" && ("Running" in status || "Stopping" in status);
}

function isStopping(status: SessionStatus): boolean {
  return typeof status === "object" && "Stopping" in status;
}

export function ControlBar({
  status,
  onStart,
  onStop,
  onAbort,
  onRemove,
}: ControlBarProps) {
  const running = isRunning(status);
  const stopping = isStopping(status);

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: "8px",
        padding: "8px 12px",
        backgroundColor: "var(--bg-secondary)",
        borderBottom: "1px solid var(--border-primary)",
      }}
    >
      {!running && (
        <button onClick={onStart} style={btnStyle("var(--accent-green)")} title="Start the autonomous coding loop">
          Start
        </button>
      )}
      {running && !stopping && (
        <button onClick={onStop} style={btnStyle("var(--accent-yellow)")} title="Stop gracefully after the current iteration finishes (commits will be pushed)">
          Stop
        </button>
      )}
      {running && (
        <button onClick={onAbort} style={btnStyle("var(--accent-red)")} title="Abort immediately — kills the AI process and stops the loop now">
          Abort
        </button>
      )}
      {!running && (
        <button onClick={onRemove} style={btnStyle("var(--text-muted)")} title="Remove this session">
          Remove
        </button>
      )}
      {stopping && (
        <span style={{ color: "var(--status-stopping)", fontSize: "12px" }}>
          Stopping after current iteration...
        </span>
      )}
    </div>
  );
}

function btnStyle(bg: string): React.CSSProperties {
  return {
    padding: "4px 12px",
    backgroundColor: bg,
    color: "#fff",
    border: "none",
    borderRadius: "6px",
    cursor: "pointer",
    fontSize: "13px",
    fontWeight: 500,
  };
}
