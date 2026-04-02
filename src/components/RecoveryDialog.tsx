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
        <h3 style={{ margin: "0 0 8px", color: "#f87171" }}>
          Action Required
        </h3>
        <p style={{ color: "#e6edf3", fontSize: 13, margin: "0 0 16px" }}>
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
                  option.id === "reset" ? "#da3633" :
                  option.id === "abort" ? "#6e7681" :
                  "#238636",
              }}
            >
              <div style={{ fontWeight: 500, fontSize: 13, color: "#e6edf3" }}>
                {option.label}
              </div>
              <div style={{ fontSize: 11, color: "#8b949e" }}>
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
  backgroundColor: "rgba(0,0,0,0.6)",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  zIndex: 1000,
};

const dialogStyle: React.CSSProperties = {
  backgroundColor: "#161b22",
  border: "1px solid #da3633",
  borderRadius: 8,
  padding: 24,
  width: 460,
  maxHeight: "80vh",
  overflow: "auto",
};

const errorStyle: React.CSSProperties = {
  backgroundColor: "#0d1117",
  border: "1px solid #30363d",
  borderRadius: 6,
  padding: 12,
  color: "#f87171",
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
  backgroundColor: "#0d1117",
  border: "1px solid #30363d",
  borderRadius: 6,
  cursor: "pointer",
  textAlign: "left",
};
