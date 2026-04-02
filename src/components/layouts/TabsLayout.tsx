import { SessionList } from "../SessionList";
import { SessionPanel } from "../SessionPanel";
import type { SessionState } from "../../lib/types";

interface TabsLayoutProps {
  sessions: SessionState[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onNewSession: () => void;
  onOpenSettings: () => void;
}

export function TabsLayout({
  sessions,
  activeId,
  onSelect,
  onNewSession,
  onOpenSettings,
}: TabsLayoutProps) {
  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100vh" }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          backgroundColor: "#0d1117",
          borderBottom: "1px solid #30363d",
        }}
      >
        <SessionList
          sessions={sessions}
          activeId={activeId}
          onSelect={onSelect}
          orientation="horizontal"
        />
        <button onClick={onNewSession} style={addTabBtnStyle} title="New session">
          +
        </button>
        <div style={{ flex: 1 }} />
        <button onClick={onOpenSettings} style={settingsBtnStyle} title="Settings">
          &#9881;
        </button>
      </div>

      <SessionPanel sessionId={activeId} />
    </div>
  );
}

const addTabBtnStyle: React.CSSProperties = {
  padding: "6px 12px",
  background: "none",
  border: "none",
  color: "#8b949e",
  cursor: "pointer",
  fontSize: 18,
};

const settingsBtnStyle: React.CSSProperties = {
  padding: "6px 12px",
  background: "none",
  border: "none",
  color: "#8b949e",
  cursor: "pointer",
  fontSize: 16,
};
