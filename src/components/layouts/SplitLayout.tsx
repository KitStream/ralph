import { SessionPanel } from "../SessionPanel";
import type { SessionState } from "../../lib/types";

interface SplitLayoutProps {
  sessions: SessionState[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onNewSession: () => void;
  onOpenSettings: () => void;
}

export function SplitLayout({
  sessions,
  onNewSession,
  onOpenSettings,
}: SplitLayoutProps) {
  const cols = sessions.length <= 1 ? 1 : sessions.length <= 4 ? 2 : 3;

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100vh" }}>
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          padding: "6px 12px",
          backgroundColor: "#0d1117",
          borderBottom: "1px solid #30363d",
        }}
      >
        <span style={{ color: "#e6edf3", fontWeight: 600, fontSize: 14 }}>
          Ralph
        </span>
        <div style={{ display: "flex", gap: 8 }}>
          <button onClick={onNewSession} style={topBtnStyle}>
            + New
          </button>
          <button onClick={onOpenSettings} style={topBtnStyle}>
            &#9881;
          </button>
        </div>
      </div>

      <div
        style={{
          flex: 1,
          display: "grid",
          gridTemplateColumns: `repeat(${cols}, 1fr)`,
          gap: 1,
          backgroundColor: "#30363d",
          overflow: "auto",
        }}
      >
        {sessions.map((session) => (
          <div
            key={session.id}
            style={{
              display: "flex",
              flexDirection: "column",
              minHeight: 300,
              backgroundColor: "#0d1117",
            }}
          >
            <SessionPanel sessionId={session.id} />
          </div>
        ))}
        {sessions.length === 0 && (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              color: "#6b7280",
              backgroundColor: "#0d1117",
              gridColumn: "1 / -1",
              minHeight: 300,
            }}
          >
            No sessions. Click "+ New" to create one.
          </div>
        )}
      </div>
    </div>
  );
}

const topBtnStyle: React.CSSProperties = {
  padding: "4px 10px",
  backgroundColor: "#21262d",
  color: "#e6edf3",
  border: "1px solid #30363d",
  borderRadius: 6,
  cursor: "pointer",
  fontSize: 12,
};
