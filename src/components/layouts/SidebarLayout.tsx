import { SessionList } from "../SessionList";
import { SessionPanel } from "../SessionPanel";
import type { SessionState } from "../../lib/types";

interface SidebarLayoutProps {
  sessions: SessionState[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onNewSession: () => void;
  onOpenSettings: () => void;
}

export function SidebarLayout({
  sessions,
  activeId,
  onSelect,
  onNewSession,
  onOpenSettings,
}: SidebarLayoutProps) {
  return (
    <div style={{ display: "flex", height: "100vh" }}>
      <div
        style={{
          width: 200,
          minWidth: 160,
          backgroundColor: "#0d1117",
          borderRight: "1px solid #30363d",
          display: "flex",
          flexDirection: "column",
        }}
      >
        <div
          style={{
            padding: "12px",
            borderBottom: "1px solid #30363d",
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
          }}
        >
          <span style={{ color: "#e6edf3", fontWeight: 600, fontSize: 14 }}>
            Ralph
          </span>
          <button onClick={onOpenSettings} style={iconBtnStyle} title="Settings">
            &#9881;
          </button>
        </div>

        <div style={{ flex: 1, overflow: "auto" }}>
          <SessionList
            sessions={sessions}
            activeId={activeId}
            onSelect={onSelect}
          />
        </div>

        <div style={{ padding: 8 }}>
          <button onClick={onNewSession} style={newBtnStyle}>
            + New Session
          </button>
        </div>
      </div>

      <SessionPanel sessionId={activeId} />
    </div>
  );
}

const iconBtnStyle: React.CSSProperties = {
  background: "none",
  border: "none",
  color: "#8b949e",
  cursor: "pointer",
  fontSize: 16,
  padding: 4,
};

const newBtnStyle: React.CSSProperties = {
  width: "100%",
  padding: "6px 0",
  backgroundColor: "#21262d",
  color: "#e6edf3",
  border: "1px solid #30363d",
  borderRadius: 6,
  cursor: "pointer",
  fontSize: 13,
};
