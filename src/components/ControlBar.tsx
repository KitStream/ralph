import type { SessionStatus } from "../lib/types";

interface ControlBarProps {
  status: SessionStatus;
  onStart: () => void;
  onResume: () => void;
  onStop: () => void;
  onCancelStop: () => void;
  onAbort: () => void;
  onClose: () => void;
}

function isRunning(status: SessionStatus): boolean {
  return typeof status === "object" && ("Running" in status || "Stopping" in status);
}

function isStopping(status: SessionStatus): boolean {
  return typeof status === "object" && "Stopping" in status;
}

function isAborted(status: SessionStatus): boolean {
  return typeof status === "object" && "Aborted" in status;
}

export function ControlBar({
  status,
  onStart,
  onResume,
  onStop,
  onCancelStop,
  onAbort,
  onClose,
}: ControlBarProps) {
  const running = isRunning(status);
  const stopping = isStopping(status);
  const aborted = isAborted(status);

  const handleClose = () => {
    if (aborted) {
      if (!window.confirm("This session was aborted mid-iteration. Closing it may discard in-progress work. Continue?")) {
        return;
      }
    }
    onClose();
  };

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
      {!running && !aborted && (
        <button onClick={onStart} style={btnStyle("var(--accent-green)")} title="Start the autonomous coding loop">
          Start
        </button>
      )}
      {aborted && (
        <button onClick={onResume} style={btnStyle("var(--accent-green)")} title="Resume the aborted session — the AI will pick up where it left off">
          Resume
        </button>
      )}
      {aborted && (
        <button onClick={onStart} style={btnStyle("var(--bg-tertiary)")} title="Start fresh, ignoring the previous AI session">
          Start Fresh
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
        <button onClick={handleClose} style={btnStyle("var(--text-muted)")} title="Close this session">
          Close
        </button>
      )}
      {stopping && (
        <>
          <button onClick={onCancelStop} style={btnStyle("var(--bg-tertiary)")} title="Cancel the stop request and keep running">
            Cancel Stop
          </button>
          <span style={{ color: "var(--status-stopping)", fontSize: "12px" }}>
            Stopping after current iteration...
          </span>
        </>
      )}
      {aborted && (
        <span style={{ color: "#f97316", fontSize: "12px" }}>
          Session was aborted. Resume to continue or Close to discard.
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
