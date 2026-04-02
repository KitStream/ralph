import { useState, useEffect } from "react";
import { useSessions } from "../hooks/useSessions";
import type { AppSettings, LayoutMode, ThemeMode } from "../lib/types";

interface SettingsDialogProps {
  open: boolean;
  onClose: () => void;
}

export function SettingsDialog({ open, onClose }: SettingsDialogProps) {
  const { state, updateSettings } = useSessions();
  const [settings, setSettings] = useState<AppSettings>(state.settings);

  useEffect(() => {
    setSettings(state.settings);
  }, [state.settings]);

  if (!open) return null;

  const handleSave = async () => {
    await updateSettings(settings);
    onClose();
  };

  const layouts: { value: LayoutMode; label: string; desc: string }[] = [
    {
      value: "Sidebar",
      label: "Sidebar",
      desc: "Vertical session list on the left, main panel on the right",
    },
    {
      value: "Tabs",
      label: "Tabs",
      desc: "Horizontal tab bar on top, session panel below",
    },
    {
      value: "Split",
      label: "Split Panes",
      desc: "All sessions visible at once in a grid",
    },
  ];

  const themes: { value: ThemeMode; label: string }[] = [
    { value: "Dark", label: "Dark" },
    { value: "Light", label: "Light" },
  ];

  return (
    <div style={overlayStyle}>
      <div style={dialogStyle}>
        <h2 style={{ margin: "0 0 16px", color: "var(--text-primary)" }}>Settings</h2>

        <div style={fieldStyle}>
          <label style={labelStyle}>Theme</label>
          <div style={{ display: "flex", gap: 8 }}>
            {themes.map((t) => (
              <label
                key={t.value}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  cursor: "pointer",
                  padding: "8px 16px",
                  borderRadius: 6,
                  border:
                    settings.theme === t.value
                      ? "1px solid var(--accent-blue)"
                      : "1px solid var(--border-primary)",
                  backgroundColor:
                    settings.theme === t.value ? "var(--bg-selected)" : "transparent",
                  color: "var(--text-primary)",
                  flex: 1,
                  justifyContent: "center",
                }}
              >
                <input
                  type="radio"
                  name="theme"
                  value={t.value}
                  checked={settings.theme === t.value}
                  onChange={() =>
                    setSettings({ ...settings, theme: t.value })
                  }
                />
                <span style={{ fontWeight: 500, fontSize: 13 }}>{t.label}</span>
              </label>
            ))}
          </div>
        </div>

        <div style={fieldStyle}>
          <label style={labelStyle}>Layout Mode</label>
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            {layouts.map((l) => (
              <label
                key={l.value}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  cursor: "pointer",
                  padding: "8px 12px",
                  borderRadius: 6,
                  border:
                    settings.layout === l.value
                      ? "1px solid var(--accent-blue)"
                      : "1px solid var(--border-primary)",
                  backgroundColor:
                    settings.layout === l.value ? "var(--bg-selected)" : "transparent",
                  color: "var(--text-primary)",
                }}
              >
                <input
                  type="radio"
                  name="layout"
                  value={l.value}
                  checked={settings.layout === l.value}
                  onChange={() =>
                    setSettings({ ...settings, layout: l.value })
                  }
                />
                <div>
                  <div style={{ fontWeight: 500, fontSize: 13 }}>{l.label}</div>
                  <div style={{ fontSize: 11, color: "var(--text-secondary)" }}>{l.desc}</div>
                </div>
              </label>
            ))}
          </div>
        </div>

        <div style={fieldStyle}>
          <label style={labelStyle}>Default AI Backend</label>
          <select
            style={inputStyle}
            value={settings.default_ai_tool}
            onChange={(e) =>
              setSettings({ ...settings, default_ai_tool: e.target.value })
            }
          >
            <option value="claude">Claude</option>
            <option value="codex">Codex</option>
            <option value="copilot">Copilot</option>
            <option value="cursor">Cursor</option>
          </select>
        </div>

        <div style={fieldStyle}>
          <label style={labelStyle}>Default Main Branch</label>
          <input
            style={inputStyle}
            value={settings.default_main_branch}
            onChange={(e) =>
              setSettings({ ...settings, default_main_branch: e.target.value })
            }
          />
        </div>

        <div style={fieldStyle}>
          <label
            style={{
              color: "var(--text-secondary)",
              display: "flex",
              alignItems: "center",
              gap: 6,
              fontSize: 13,
            }}
          >
            <input
              type="checkbox"
              checked={settings.default_tagging_enabled}
              onChange={(e) =>
                setSettings({
                  ...settings,
                  default_tagging_enabled: e.target.checked,
                })
              }
            />
            Enable tagging by default
          </label>
        </div>

        <div
          style={{
            display: "flex",
            justifyContent: "flex-end",
            gap: 8,
            marginTop: 16,
          }}
        >
          <button onClick={onClose} style={cancelBtnStyle}>
            Cancel
          </button>
          <button onClick={handleSave} style={saveBtnStyle}>
            Save
          </button>
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
  border: "1px solid var(--border-primary)",
  borderRadius: 8,
  padding: 24,
  width: 420,
  maxHeight: "90vh",
  overflow: "auto",
};

const fieldStyle: React.CSSProperties = { marginBottom: 16 };

const labelStyle: React.CSSProperties = {
  display: "block",
  color: "var(--text-secondary)",
  fontSize: 12,
  marginBottom: 6,
};

const inputStyle: React.CSSProperties = {
  width: "100%",
  padding: "6px 10px",
  backgroundColor: "var(--bg-primary)",
  border: "1px solid var(--border-primary)",
  borderRadius: 6,
  color: "var(--text-primary)",
  fontSize: 13,
  boxSizing: "border-box",
};

const cancelBtnStyle: React.CSSProperties = {
  padding: "6px 16px",
  backgroundColor: "var(--bg-tertiary)",
  color: "var(--text-primary)",
  border: "1px solid var(--border-primary)",
  borderRadius: 6,
  cursor: "pointer",
  fontSize: 13,
};

const saveBtnStyle: React.CSSProperties = {
  padding: "6px 16px",
  backgroundColor: "var(--accent-green)",
  color: "#fff",
  border: "none",
  borderRadius: 6,
  cursor: "pointer",
  fontSize: 13,
  fontWeight: 500,
};
