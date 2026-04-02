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
        backgroundColor: "#161b22",
        borderBottom: "1px solid #30363d",
      }}
    >
      {!running && (
        <button onClick={onStart} style={btnStyle("#238636")} title="Start the autonomous coding loop">
          Start
        </button>
      )}
      {running && !stopping && (
        <button onClick={onStop} style={btnStyle("#d29922")} title="Stop gracefully after the current iteration finishes (commits will be pushed)">
          Stop
        </button>
      )}
      {running && (
        <button onClick={onAbort} style={btnStyle("#da3633")} title="Abort immediately — kills the AI process and stops the loop now">
          Abort
        </button>
      )}
      {!running && (
        <button onClick={onRemove} style={btnStyle("#6e7681")} title="Remove this session">
          Remove
        </button>
      )}
      {stopping && (
        <span style={{ color: "#fbbf24", fontSize: "12px" }}>
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
