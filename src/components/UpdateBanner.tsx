import type { UpdateStatus } from "../hooks/useAppUpdate";

interface UpdateBannerProps {
  status: UpdateStatus;
  onInstall: () => void;
  onDismiss: () => void;
}

export function UpdateBanner({ status, onInstall, onDismiss }: UpdateBannerProps) {
  if (status.kind === "idle" || status.kind === "checking" || status.kind === "upToDate") {
    return null;
  }

  let content: React.ReactNode;
  if (status.kind === "available") {
    content = (
      <>
        <span>Update available: v{status.version}</span>
        <div style={btnGroupStyle}>
          <button onClick={onInstall} style={primaryBtnStyle}>
            Install and restart
          </button>
          <button onClick={onDismiss} style={secondaryBtnStyle}>
            Later
          </button>
        </div>
      </>
    );
  } else if (status.kind === "downloading") {
    const pct =
      status.total && status.total > 0
        ? Math.min(100, Math.round((status.downloaded / status.total) * 100))
        : null;
    content = (
      <span>
        Downloading update… {pct !== null ? `${pct}%` : formatBytes(status.downloaded)}
      </span>
    );
  } else if (status.kind === "ready") {
    content = <span>Update installed. Restarting…</span>;
  } else {
    content = (
      <>
        <span>Update failed: {status.message}</span>
        <button onClick={onDismiss} style={secondaryBtnStyle}>
          Dismiss
        </button>
      </>
    );
  }

  return <div style={bannerStyle}>{content}</div>;
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

const bannerStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 12,
  padding: "6px 12px",
  backgroundColor: "var(--bg-tertiary)",
  borderBottom: "1px solid var(--border-primary)",
  color: "var(--text-primary)",
  fontSize: 13,
};

const btnGroupStyle: React.CSSProperties = {
  display: "flex",
  gap: 6,
};

const primaryBtnStyle: React.CSSProperties = {
  padding: "4px 10px",
  backgroundColor: "var(--accent-blue)",
  color: "#fff",
  border: "none",
  borderRadius: 6,
  cursor: "pointer",
  fontSize: 12,
};

const secondaryBtnStyle: React.CSSProperties = {
  padding: "4px 10px",
  backgroundColor: "transparent",
  color: "var(--text-primary)",
  border: "1px solid var(--border-primary)",
  borderRadius: 6,
  cursor: "pointer",
  fontSize: 12,
};
