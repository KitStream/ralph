import { sendRecoveryAction } from "../lib/commands";
import type { RecoveryRequest } from "../lib/types";

interface RecoveryDialogProps {
  request: RecoveryRequest;
}

export function RecoveryDialog({ request }: RecoveryDialogProps) {
  const handleAction = async (actionId: string) => {
    await sendRecoveryAction(request.sessionId, actionId);
  };

  return (
    <div style={overlayStyle}>
      <div style={dialogStyle}>
        <h3 style={{ margin: "0 0 8px", color: "var(--status-failed)" }}>
          Action Required
        </h3>
        <p style={{ color: "var(--text-primary)", fontSize: 13, margin: "0 0 16px" }}>
          The session encountered an error and needs your input to continue:
        </p>
        <pre style={errorStyle}>{request.error}</pre>
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          {request.options.map((option) => (
            <button
              key={option.id}
              onClick={() => handleAction(option.id)}
              style={{
                ...optionBtnStyle,
                borderColor:
                  option.id === "reset" ? "var(--accent-red)" :
                  option.id === "abort" ? "var(--text-muted)" :
                  "var(--accent-green)",
              }}
            >
              <div style={{ fontWeight: 500, fontSize: 13, color: "var(--text-primary)" }}>
                {option.label}
              </div>
              <div style={{ fontSize: 11, color: "var(--text-secondary)" }}>
                {option.description}
              </div>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

const overlayStyle: React.CSSProperties = {
  position: "fixed",
  top: 0,
  left: 0,
  right: 0,
  bottom: 0,
  backgroundColor: "var(--overlay-bg)",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  zIndex: 1000,
};

const dialogStyle: React.CSSProperties = {
  backgroundColor: "var(--bg-secondary)",
  border: "1px solid var(--accent-red)",
  borderRadius: 8,
  padding: 24,
  width: 460,
  maxHeight: "80vh",
  overflow: "auto",
};

const errorStyle: React.CSSProperties = {
  backgroundColor: "var(--bg-primary)",
  border: "1px solid var(--border-primary)",
  borderRadius: 6,
  padding: 12,
  color: "var(--status-failed)",
  fontSize: 12,
  fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
  whiteSpace: "pre-wrap",
  wordBreak: "break-all",
  margin: "0 0 16px",
  maxHeight: 120,
  overflow: "auto",
};

const optionBtnStyle: React.CSSProperties = {
  padding: "10px 14px",
  backgroundColor: "var(--bg-primary)",
  border: "1px solid var(--border-primary)",
  borderRadius: 6,
  cursor: "pointer",
  textAlign: "left",
};
