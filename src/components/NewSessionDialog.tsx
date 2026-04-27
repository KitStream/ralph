import { useState, useEffect } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useDiscovery } from "../hooks/useDiscovery";
import { useSessions } from "../hooks/useSessions";
import { getAvailableTools, listBackendModels } from "../lib/commands";
import type { AiToolInfo, BackendModelConfig } from "../lib/types";

interface NewSessionDialogProps {
  open: boolean;
  onClose: () => void;
}

export function NewSessionDialog({
  open: isOpen,
  onClose,
}: NewSessionDialogProps) {
  const { createSession, startSession, state } = useSessions();
  const { modes, discover } = useDiscovery();
  const [tools, setTools] = useState<AiToolInfo[]>([]);

  const [projectDir, setProjectDir] = useState("");
  const [selectedMode, setSelectedMode] = useState("");
  const [branchName, setBranchName] = useState("");
  const [mainBranch, setMainBranch] = useState(
    state.settings.default_main_branch
  );
  const [preamble, setPreamble] = useState(
    state.settings.default_preamble ?? ""
  );
  const [taggingEnabled, setTaggingEnabled] = useState(
    state.settings.default_tagging_enabled
  );
  const [aiTool, setAiTool] = useState(state.settings.default_ai_tool);
  const [model, setModel] = useState<string | null>(null);
  const [modelConfig, setModelConfig] = useState<BackendModelConfig | null>(null);
  const [modelLoading, setModelLoading] = useState(false);
  const [customModel, setCustomModel] = useState("");
  const [useCustom, setUseCustom] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [autoStart, setAutoStart] = useState(true);

  useEffect(() => {
    getAvailableTools().then(setTools);
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [isOpen, onClose]);

  // Reset form defaults when dialog opens or settings load
  useEffect(() => {
    if (isOpen) {
      setAiTool(state.settings.default_ai_tool);
      setMainBranch(state.settings.default_main_branch);
      setTaggingEnabled(state.settings.default_tagging_enabled);
      setPreamble(state.settings.default_preamble ?? "");
    }
  }, [isOpen, state.settings]);

  useEffect(() => {
    if (aiTool) {
      setModelLoading(true);
      setModel(null);
      setUseCustom(false);
      setCustomModel("");
      listBackendModels(aiTool)
        .then((config) => {
          setModelConfig(config);
          setModel(config.current_model);
        })
        .catch(() => {
          setModelConfig({ models: [], supports_freeform: true, current_model: null });
        })
        .finally(() => setModelLoading(false));
    }
  }, [aiTool]);

  useEffect(() => {
    if (projectDir) {
      discover(projectDir);
    }
  }, [projectDir, isOpen, discover]);

  useEffect(() => {
    if (modes.length > 0 && !selectedMode) {
      const defaultMode = state.settings.default_mode;
      const match = defaultMode && modes.find((m) => m.name === defaultMode);
      setSelectedMode(match ? match.name : modes[0].name);
    }
  }, [modes, selectedMode, state.settings.default_mode]);

  useEffect(() => {
    if (selectedMode) {
      setBranchName(`ralph-${selectedMode}`);
    }
  }, [selectedMode]);

  if (!isOpen) return null;

  const handlePickDir = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (selected) {
      setProjectDir(selected as string);
    }
  };

  const handleCreate = async () => {
    const mode = modes.find((m) => m.name === selectedMode);
    if (!mode) return;
    if (autoStart && !toolAvailable) return;

    const selectedModel = useCustom ? (customModel || null) : model;
    const id = await createSession({
      project_dir: projectDir,
      mode: selectedMode,
      prompt_file: mode.prompt_file,
      branch_name: branchName,
      main_branch: mainBranch,
      preamble,
      tagging_enabled: taggingEnabled,
      ai_tool: aiTool,
      model: selectedModel,
    });

    if (autoStart) {
      await startSession(id);
    }

    onClose();
  };

  const selectedToolInfo = tools.find((t) => t.id === aiTool);
  const toolAvailable = selectedToolInfo?.available ?? false;
  const toolBlocksStart = autoStart && !toolAvailable;
  const canCreate = Boolean(
    projectDir && selectedMode && branchName && !toolBlocksStart
  );

  return (
    <div style={overlayStyle}>
      <div style={dialogStyle}>
        <h2 style={{ margin: "0 0 16px", color: "var(--text-primary)" }}>New Session</h2>

        <div style={fieldStyle}>
          <label style={labelStyle}>Project Directory</label>
          {state.settings.recent_project_dirs.length > 0 && !projectDir && (
            <div style={{ marginBottom: 6 }}>
              <span style={{ fontSize: 11, color: "var(--text-muted)" }}>Recent:</span>
              <div style={{ display: "flex", flexWrap: "wrap", gap: 4, marginTop: 4 }}>
                {state.settings.recent_project_dirs.map((dir) => (
                  <button
                    key={dir}
                    onClick={() => setProjectDir(dir)}
                    style={{
                      padding: "2px 8px",
                      backgroundColor: "var(--bg-tertiary)",
                      color: "var(--text-secondary)",
                      border: "1px solid var(--border-primary)",
                      borderRadius: 4,
                      cursor: "pointer",
                      fontSize: 11,
                      maxWidth: "100%",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                    title={dir}
                  >
                    {dir.split("/").slice(-2).join("/")}
                  </button>
                ))}
              </div>
            </div>
          )}
          <div style={{ display: "flex", gap: 8 }}>
            <div style={{ flex: 1, position: "relative" }}>
              <input
                style={{ ...inputStyle, paddingRight: projectDir ? 28 : 10 }}
                value={projectDir}
                onChange={(e) => setProjectDir(e.target.value)}
                placeholder="/path/to/project"
              />
              {projectDir && (
                <button
                  onClick={() => setProjectDir("")}
                  style={clearBtnStyle}
                  title="Clear"
                >
                  ×
                </button>
              )}
            </div>
            <button onClick={handlePickDir} style={pickBtnStyle}>
              Browse
            </button>
          </div>
        </div>

        <div style={fieldStyle}>
          <label style={labelStyle}>Mode</label>
          <select
            style={inputStyle}
            value={selectedMode}
            onChange={(e) => setSelectedMode(e.target.value)}
          >
            {modes.length === 0 && (
              <option value="">
                {projectDir ? "No PROMPT-*.md files found" : "Select a project first"}
              </option>
            )}
            {modes.map((m) => (
              <option key={m.name} value={m.name}>
                {m.name}
              </option>
            ))}
          </select>
        </div>

        <div style={fieldStyle}>
          <label style={labelStyle}>AI Backend</label>
          <select
            style={inputStyle}
            value={aiTool}
            onChange={(e) => setAiTool(e.target.value)}
          >
            {tools.map((t) => (
              <option key={t.id} value={t.id} disabled={!t.available}>
                {t.name}{!t.available ? " (not installed)" : ""}
              </option>
            ))}
          </select>
          {!toolAvailable && selectedToolInfo && (
            <div
              style={{
                marginTop: 6,
                fontSize: 11,
                color: "var(--accent-red, #e06c75)",
              }}
            >
              {selectedToolInfo.name} was not detected on PATH. Configure its
              binary path in Settings → Tool Binary Paths, or install the CLI,
              before starting a session.
            </div>
          )}
        </div>

        <div style={{ marginBottom: 12 }}>
          <button
            onClick={() => setShowAdvanced(!showAdvanced)}
            style={{
              background: "none",
              border: "none",
              color: "var(--text-secondary)",
              cursor: "pointer",
              fontSize: 12,
              padding: "4px 0",
              display: "flex",
              alignItems: "center",
              gap: 4,
            }}
          >
            <span style={{ display: "inline-block", transform: showAdvanced ? "rotate(90deg)" : "none", transition: "transform 0.15s" }}>
              &#9654;
            </span>
            Advanced
          </button>
          {showAdvanced && (
            <div style={{ paddingLeft: 12, borderLeft: "2px solid var(--border-primary)", marginTop: 8 }}>
              <div style={fieldStyle}>
                <label style={labelStyle}>Model</label>
                {modelLoading ? (
                  <div style={{ color: "var(--text-muted)", fontSize: 12, padding: "6px 0" }}>
                    Loading models...
                  </div>
                ) : modelConfig && (modelConfig.models.length > 0 || modelConfig.supports_freeform) ? (
                  <>
                    {modelConfig.models.length > 0 && !useCustom && (
                      <select
                        style={inputStyle}
                        value={model ?? ""}
                        onChange={(e) => {
                          const val = e.target.value;
                          if (val === "__custom__") {
                            setUseCustom(true);
                            setModel(null);
                          } else {
                            setModel(val || null);
                          }
                        }}
                      >
                        <option value="">Default</option>
                        {modelConfig.models.map((m) => (
                          <option key={m.id} value={m.id}>
                            {m.label}
                          </option>
                        ))}
                        {modelConfig.supports_freeform && (
                          <option value="__custom__">Custom...</option>
                        )}
                      </select>
                    )}
                    {(useCustom || (modelConfig.models.length === 0 && modelConfig.supports_freeform)) && (
                      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                        <input
                          style={{ ...inputStyle, flex: 1 }}
                          value={customModel}
                          onChange={(e) => setCustomModel(e.target.value)}
                          placeholder="Enter model name..."
                          autoCapitalize="off"
                          autoCorrect="off"
                          autoComplete="off"
                          spellCheck={false}
                        />
                        {modelConfig.models.length > 0 && (
                          <button
                            onClick={() => {
                              setUseCustom(false);
                              setCustomModel("");
                              setModel(modelConfig.current_model);
                            }}
                            style={{
                              padding: "4px 8px",
                              backgroundColor: "var(--bg-tertiary)",
                              color: "var(--text-secondary)",
                              border: "1px solid var(--border-primary)",
                              borderRadius: 4,
                              cursor: "pointer",
                              fontSize: 11,
                              whiteSpace: "nowrap",
                            }}
                          >
                            Back to list
                          </button>
                        )}
                      </div>
                    )}
                  </>
                ) : (
                  <div style={{ color: "var(--text-muted)", fontSize: 12, padding: "6px 0" }}>
                    No model selection available
                  </div>
                )}
              </div>

              <div style={fieldStyle}>
                <label style={labelStyle}>Branch Name</label>
                <input
                  style={inputStyle}
                  value={branchName}
                  onChange={(e) => setBranchName(e.target.value)}
                />
              </div>

              <div style={fieldStyle}>
                <label style={labelStyle}>Main Branch</label>
                <input
                  style={inputStyle}
                  value={mainBranch}
                  onChange={(e) => setMainBranch(e.target.value)}
                />
              </div>
            </div>
          )}
        </div>

        <div style={fieldStyle}>
          <label style={labelStyle}>Preamble</label>
          {state.settings.recent_preambles.length > 0 && (
            <select
              style={{ ...inputStyle, marginBottom: 6 }}
              value={preamble}
              onChange={(e) => setPreamble(e.target.value)}
            >
              <option value="">-- No preamble --</option>
              {state.settings.recent_preambles.map((p, i) => (
                <option key={i} value={p}>
                  {p.length > 80 ? p.slice(0, 80) + "..." : p}
                </option>
              ))}
            </select>
          )}
          <div style={{ position: "relative" }}>
            <textarea
              style={{ ...inputStyle, minHeight: 60, resize: "vertical", paddingRight: preamble ? 28 : 10 }}
              value={preamble}
              onChange={(e) => setPreamble(e.target.value)}
              placeholder="Optional text prepended to the prompt..."
            />
            {preamble && (
              <button
                onClick={() => setPreamble("")}
                style={{ ...clearBtnStyle, top: 6 }}
                title="Clear"
              >
                ×
              </button>
            )}
          </div>
        </div>

        <div style={{ display: "flex", gap: 16, marginBottom: 16 }}>
          <label style={{ color: "var(--text-secondary)", display: "flex", alignItems: "center", gap: 6 }}>
            <input
              type="checkbox"
              checked={taggingEnabled}
              onChange={(e) => setTaggingEnabled(e.target.checked)}
            />
            Enable tagging
          </label>
          <label style={{ color: "var(--text-secondary)", display: "flex", alignItems: "center", gap: 6 }}>
            <input
              type="checkbox"
              checked={autoStart}
              onChange={(e) => setAutoStart(e.target.checked)}
            />
            Start immediately
          </label>
        </div>

        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
          <button onClick={onClose} style={cancelBtnStyle}>
            Cancel
          </button>
          <button
            onClick={handleCreate}
            disabled={!canCreate}
            style={{
              ...createBtnStyle,
              opacity: canCreate ? 1 : 0.5,
              cursor: canCreate ? "pointer" : "not-allowed",
            }}
          >
            {autoStart ? "Create & Start" : "Create"}
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
  width: 480,
  maxHeight: "80vh",
  overflow: "auto",
};

const fieldStyle: React.CSSProperties = {
  marginBottom: 12,
};

const labelStyle: React.CSSProperties = {
  display: "block",
  color: "var(--text-secondary)",
  fontSize: 12,
  marginBottom: 4,
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

const clearBtnStyle: React.CSSProperties = {
  position: "absolute",
  right: 6,
  top: "50%",
  transform: "translateY(-50%)",
  background: "none",
  border: "none",
  color: "var(--text-muted)",
  cursor: "pointer",
  fontSize: 16,
  padding: "0 4px",
  lineHeight: 1,
};

const pickBtnStyle: React.CSSProperties = {
  padding: "6px 12px",
  backgroundColor: "var(--bg-tertiary)",
  color: "var(--text-primary)",
  border: "1px solid var(--border-primary)",
  borderRadius: 6,
  cursor: "pointer",
  fontSize: 13,
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

const createBtnStyle: React.CSSProperties = {
  padding: "6px 16px",
  backgroundColor: "var(--accent-green)",
  color: "#fff",
  border: "none",
  borderRadius: 6,
  cursor: "pointer",
  fontSize: 13,
  fontWeight: 500,
};
