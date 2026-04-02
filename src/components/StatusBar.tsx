import type { SessionStatus, SessionStep } from "../lib/types";

interface StatusBarProps {
  status: SessionStatus;
  iterationCount: number;
  lastTag: string | null;
  mode: string;
  aiTool: string;
}

function getStepLabel(step: SessionStep): string {
  const labels: Record<SessionStep, string> = {
    Idle: "Idle",
    Checkout: "Checking out",
    RebasePreAi: "Rebasing (pre-AI)",
    RunningAi: "Running AI",
    PushBranch: "Pushing branch",
    RebasePostAi: "Rebasing (post-AI)",
    PushToMain: "Pushing to main",
    Tagging: "Tagging",
    RecoveringGit: "Recovering git",
  };
  return labels[step] || step;
}

function getStatusInfo(status: SessionStatus): {
  label: string;
  color: string;
  step?: string;
  iteration?: number;
} {
  if (status === "Created") return { label: "Ready", color: "var(--status-idle)" };
  if (status === "Stopped") return { label: "Stopped", color: "var(--status-idle)" };
  if (typeof status === "object") {
    if ("Running" in status)
      return {
        label: "Running",
        color: "var(--status-running)",
        step: getStepLabel(status.Running.step),
        iteration: status.Running.iteration,
      };
    if ("Stopping" in status)
      return {
        label: "Stopping",
        color: "var(--status-stopping)",
        step: getStepLabel(status.Stopping.step),
        iteration: status.Stopping.iteration,
      };
    if ("Failed" in status)
      return { label: `Failed: ${status.Failed.error}`, color: "var(--status-failed)" };
  }
  return { label: "Unknown", color: "var(--status-idle)" };
}

export function StatusBar({
  status,
  iterationCount,
  lastTag,
  mode,
  aiTool,
}: StatusBarProps) {
  const info = getStatusInfo(status);

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: "16px",
        padding: "6px 12px",
        backgroundColor: "var(--bg-secondary)",
        borderTop: "1px solid var(--border-primary)",
        fontSize: "12px",
        color: "var(--text-secondary)",
      }}
    >
      <span>
        <span
          style={{
            display: "inline-block",
            width: 8,
            height: 8,
            borderRadius: "50%",
            backgroundColor: info.color,
            marginRight: 6,
          }}
        />
        {info.label}
      </span>
      {info.step && <span>Step: {info.step}</span>}
      <span>Mode: {mode}</span>
      <span>Backend: {aiTool}</span>
      <span>Iterations: {iterationCount}</span>
      {lastTag && <span>Last tag: {lastTag}</span>}
    </div>
  );
}
