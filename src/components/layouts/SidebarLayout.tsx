import { useCallback, useRef, useState } from "react";
import { SessionList } from "../SessionList";
import { SessionPanel } from "../SessionPanel";
import type { SessionState } from "../../lib/types";

const MIN_WIDTH = 120;
const MAX_WIDTH = 500;
const DEFAULT_WIDTH = 200;

interface SidebarLayoutProps {
  sessions: SessionState[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onNewSession: () => void;
  onOpenSettings: () => void;
  appVersion: string;
}

export function SidebarLayout({
  sessions,
  activeId,
  onSelect,
  onNewSession,
  onOpenSettings,
  appVersion,
}: SidebarLayoutProps) {
  const [sidebarWidth, setSidebarWidth] = useState(DEFAULT_WIDTH);
  const dragging = useRef(false);

  const onMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      dragging.current = true;

      const onMouseMove = (ev: MouseEvent) => {
        if (!dragging.current) return;
        const newWidth = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, ev.clientX));
        setSidebarWidth(newWidth);
      };

      const onMouseUp = () => {
        dragging.current = false;
        document.removeEventListener("mousemove", onMouseMove);
        document.removeEventListener("mouseup", onMouseUp);
        document.body.style.cursor = "";
        document.body.style.userSelect = "";
      };

      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";
      document.addEventListener("mousemove", onMouseMove);
      document.addEventListener("mouseup", onMouseUp);
    },
    [],
  );

  return (
    <div style={{ display: "flex", height: "100vh" }}>
      <div
        style={{
          width: sidebarWidth,
          minWidth: MIN_WIDTH,
          maxWidth: MAX_WIDTH,
          backgroundColor: "var(--bg-primary)",
          display: "flex",
          flexDirection: "column",
          flexShrink: 0,
        }}
      >
        <div
          style={{
            padding: "12px",
            borderBottom: "1px solid var(--border-primary)",
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
          }}
        >
          <span style={{ color: "var(--text-primary)", fontWeight: 600, fontSize: 14 }}>
            Ralph{appVersion && <span style={versionStyle}>v{appVersion}</span>}
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

      <div onMouseDown={onMouseDown} style={resizeHandleStyle} />

      <SessionPanel sessionId={activeId} />
    </div>
  );
}

const resizeHandleStyle: React.CSSProperties = {
  width: 4,
  cursor: "col-resize",
  backgroundColor: "transparent",
  borderRight: "1px solid var(--border-primary)",
  flexShrink: 0,
};

const versionStyle: React.CSSProperties = {
  marginLeft: 6,
  fontWeight: 400,
  fontSize: 11,
  color: "var(--text-muted)",
};

const iconBtnStyle: React.CSSProperties = {
  background: "none",
  border: "none",
  color: "var(--text-secondary)",
  cursor: "pointer",
  fontSize: 16,
  padding: 4,
};

const newBtnStyle: React.CSSProperties = {
  width: "100%",
  padding: "6px 0",
  backgroundColor: "var(--bg-tertiary)",
  color: "var(--text-primary)",
  border: "1px solid var(--border-primary)",
  borderRadius: 6,
  cursor: "pointer",
  fontSize: 13,
};
