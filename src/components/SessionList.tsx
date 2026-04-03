import type { SessionState, SessionStatus } from "../lib/types";

interface SessionListProps {
  sessions: SessionState[];
  activeId: string | null;
  onSelect: (id: string) => void;
  orientation?: "vertical" | "horizontal";
}

function statusDotColor(status: SessionStatus): string {
  if (status === "Created") return "var(--status-idle)";
  if (status === "Stopped") return "var(--status-idle)";
  if (typeof status === "object") {
    if ("Running" in status) return "var(--status-running)";
    if ("Stopping" in status) return "var(--status-stopping)";
    if ("Aborted" in status) return "#f97316";
    if ("Failed" in status) return "var(--status-failed)";
  }
  return "var(--status-idle)";
}

function isRunning(status: SessionStatus): boolean {
  return typeof status === "object" && ("Running" in status || "Stopping" in status);
}

export function SessionList({
  sessions,
  activeId,
  onSelect,
  orientation = "vertical",
}: SessionListProps) {
  const isHorizontal = orientation === "horizontal";

  return (
    <div
      style={{
        display: "flex",
        flexDirection: isHorizontal ? "row" : "column",
        gap: isHorizontal ? 0 : 2,
        overflow: "auto",
      }}
    >
      {sessions.map((session) => {
        const isActive = session.id === activeId;
        return (
          <div
            key={session.id}
            onClick={() => onSelect(session.id)}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              padding: isHorizontal ? "6px 12px" : "8px 12px",
              cursor: "pointer",
              backgroundColor: isActive ? "var(--bg-selected)" : "transparent",
              borderBottom: isHorizontal
                ? isActive
                  ? "2px solid var(--accent-blue)"
                  : "2px solid transparent"
                : undefined,
              borderLeft: !isHorizontal
                ? isActive
                  ? "3px solid var(--accent-blue)"
                  : "3px solid transparent"
                : undefined,
              color: "var(--text-primary)",
              fontSize: 13,
              whiteSpace: "nowrap",
              minWidth: isHorizontal ? 100 : undefined,
            }}
          >
            <span
              style={{
                width: 8,
                height: 8,
                borderRadius: "50%",
                backgroundColor: statusDotColor(session.status),
                flexShrink: 0,
                animation: isRunning(session.status)
                  ? "pulse 2s infinite"
                  : undefined,
              }}
            />
            <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>
              {session.config.mode}
            </span>
            <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
              {session.config.ai_tool.charAt(0).toUpperCase() + session.config.ai_tool.slice(1)}
            </span>
            {session.iterationCount > 0 && (
              <span
                style={{
                  fontSize: 11,
                  color: "var(--text-secondary)",
                  backgroundColor: "var(--badge-bg)",
                  padding: "1px 6px",
                  borderRadius: 10,
                }}
              >
                #{session.iterationCount}
              </span>
            )}
          </div>
        );
      })}
    </div>
  );
}
