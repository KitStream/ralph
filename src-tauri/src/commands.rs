use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use ralph_core::discovery;
use ralph_core::events::{RecoveryAction, SessionEvent, SessionEventPayload};
use ralph_core::provider::{create_provider, AiTool, BackendModelConfig};
use ralph_core::session::manager::SessionManager;
use ralph_core::session::state::{SessionConfig, SessionId};

fn make_emit(
    app: AppHandle,
    manager: Arc<SessionManager>,
) -> Arc<dyn Fn(SessionEvent) + Send + Sync> {
    Arc::new(move |event: SessionEvent| {
        app.emit("session-event", &event).ok();
        let mgr = manager.clone();
        let evt = event.clone();
        tokio::spawn(async move { mgr.handle_event(&evt).await });
    })
}

#[derive(Debug, Serialize)]
pub struct ModeInfo {
    pub name: String,
    pub prompt_file: String,
    pub preview: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub project_dir: String,
    pub mode: String,
    pub prompt_file: String,
    pub branch_name: String,
    pub main_branch: String,
    pub preamble: String,
    pub tagging_enabled: bool,
    pub ai_tool: String,
    pub model: Option<String>,
}

#[tauri::command]
pub async fn discover_modes(project_dir: String) -> Result<Vec<ModeInfo>, String> {
    let path = expand_tilde(&project_dir);
    if !path.exists() {
        return Err(format!("Directory does not exist: {}", project_dir));
    }

    let modes = discovery::discover_modes(&[path.as_path()]);
    Ok(modes
        .into_iter()
        .map(|m| ModeInfo {
            name: m.name,
            prompt_file: m.prompt_file.to_string_lossy().to_string(),
            preview: m.preview,
        })
        .collect())
}

#[derive(Debug, Serialize)]
pub struct CreatedSession {
    pub id: String,
    /// Canonical project_dir (tilde-expanded, symlinks resolved).
    /// Returned so the frontend's in-memory session state matches what
    /// the backend stored, which is the prefix used in emitted log paths.
    pub project_dir: String,
}

#[tauri::command]
pub async fn create_session(
    manager: State<'_, Arc<SessionManager>>,
    request: CreateSessionRequest,
) -> Result<CreatedSession, String> {
    let ai_tool: AiTool = request.ai_tool.parse().map_err(|e: String| e)?;

    let config = SessionConfig {
        project_dir: expand_tilde(&request.project_dir),
        mode: request.mode,
        prompt_file: PathBuf::from(request.prompt_file),
        branch_name: request.branch_name,
        main_branch: request.main_branch,
        preamble: request.preamble,
        tagging_enabled: request.tagging_enabled,
        ai_tool,
        model: request.model,
    };

    let project_dir = config.project_dir.to_string_lossy().to_string();
    let id = manager.create_session(config).await;
    Ok(CreatedSession {
        id: id.to_string(),
        project_dir,
    })
}

fn ensure_tool_available(tool: &AiTool) -> Result<(), String> {
    let (id, default_cmd) = match tool {
        AiTool::Claude => ("claude", "claude"),
        AiTool::Codex => ("codex", "codex"),
        AiTool::Copilot => ("copilot", "copilot"),
        AiTool::Cursor => ("cursor", "cursor-agent"),
    };
    if tool_available(id, default_cmd) {
        Ok(())
    } else {
        Err(format!(
            "The '{}' CLI was not detected. Install it or set an explicit \
             binary path in Settings → Tool Binary Paths before starting a session.",
            default_cmd
        ))
    }
}

#[tauri::command]
pub async fn start_session(
    manager: State<'_, Arc<SessionManager>>,
    app: AppHandle,
    session_id: String,
) -> Result<(), String> {
    let id = parse_session_id(&session_id)?;
    let info = manager
        .get_session(&id)
        .await
        .ok_or_else(|| "Session not found".to_string())?;
    ensure_tool_available(&info.config.ai_tool)?;
    let emit = make_emit(app, manager.inner().clone());
    manager
        .start_session(&id, emit)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn resume_session(
    manager: State<'_, Arc<SessionManager>>,
    app: AppHandle,
    session_id: String,
) -> Result<(), String> {
    let id = parse_session_id(&session_id)?;
    let info = manager
        .get_session(&id)
        .await
        .ok_or_else(|| "Session not found".to_string())?;
    ensure_tool_available(&info.config.ai_tool)?;
    let emit = make_emit(app, manager.inner().clone());
    manager
        .resume_session(&id, emit)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_session(
    manager: State<'_, Arc<SessionManager>>,
    app: AppHandle,
    session_id: String,
) -> Result<(), String> {
    let id = parse_session_id(&session_id)?;
    let status = manager.stop_session(&id).await.map_err(|e| e.to_string())?;
    app.emit(
        "session-event",
        &SessionEvent {
            session_id: session_id.clone(),
            payload: SessionEventPayload::StatusChanged { status },
        },
    )
    .ok();
    Ok(())
}

#[tauri::command]
pub async fn cancel_stop_session(
    manager: State<'_, Arc<SessionManager>>,
    app: AppHandle,
    session_id: String,
) -> Result<(), String> {
    let id = parse_session_id(&session_id)?;
    let status = manager
        .cancel_stop_session(&id)
        .await
        .map_err(|e| e.to_string())?;
    app.emit(
        "session-event",
        &SessionEvent {
            session_id: session_id.clone(),
            payload: SessionEventPayload::StatusChanged { status },
        },
    )
    .ok();
    Ok(())
}

#[tauri::command]
pub async fn abort_session(
    manager: State<'_, Arc<SessionManager>>,
    app: AppHandle,
    session_id: String,
) -> Result<(), String> {
    let id = parse_session_id(&session_id)?;
    let status = manager
        .abort_session(&id)
        .await
        .map_err(|e| e.to_string())?;
    // Emit the Aborted status to the frontend so it can show Resume
    app.emit(
        "session-event",
        &SessionEvent {
            session_id: session_id.clone(),
            payload: SessionEventPayload::StatusChanged { status },
        },
    )
    .ok();
    Ok(())
}

#[tauri::command]
pub async fn remove_session(
    manager: State<'_, Arc<SessionManager>>,
    session_id: String,
) -> Result<(), String> {
    let id = parse_session_id(&session_id)?;
    manager.remove_session(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_sessions(
    manager: State<'_, Arc<SessionManager>>,
) -> Result<Vec<ralph_core::session::state::SessionInfo>, String> {
    Ok(manager.list_sessions().await)
}

#[tauri::command]
pub async fn list_log_iterations(
    manager: State<'_, Arc<SessionManager>>,
    session_id: String,
) -> Result<Vec<ralph_core::session::log_store::IterationSummary>, String> {
    Ok(manager.list_iterations(&session_id))
}

#[tauri::command]
pub async fn read_log_iteration(
    manager: State<'_, Arc<SessionManager>>,
    session_id: String,
    iteration: u32,
) -> Result<Vec<ralph_core::session::log_store::LogRecord>, String> {
    Ok(manager.read_iteration(&session_id, iteration))
}

#[tauri::command]
pub async fn read_log_iteration_view(
    manager: State<'_, Arc<SessionManager>>,
    session_id: String,
    iteration: u32,
) -> Result<Vec<ralph_core::session::view::ViewLogEntry>, String> {
    Ok(manager.read_iteration_view(&session_id, iteration).await)
}

#[derive(Debug, Serialize)]
pub struct AiToolInfo {
    pub id: String,
    pub name: String,
    pub available: bool,
}

fn is_on_path(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

#[derive(Debug, Serialize)]
pub struct ToolPathInfo {
    pub id: String,
    pub command: String,
    pub detected_path: Option<String>,
}

#[tauri::command]
pub fn detect_tool_paths() -> Vec<ToolPathInfo> {
    let entries = [
        ("claude", "claude"),
        ("codex", "codex"),
        ("copilot", "copilot"),
        ("cursor", "cursor-agent"),
    ];
    entries
        .iter()
        .map(|(id, cmd)| ToolPathInfo {
            id: (*id).to_string(),
            command: (*cmd).to_string(),
            detected_path: which::which(cmd)
                .ok()
                .map(|p| p.to_string_lossy().into_owned()),
        })
        .collect()
}

fn tool_available(tool_id: &str, default_cmd: &str) -> bool {
    let resolved = ralph_core::provider::resolve_tool_command(tool_id, default_cmd);
    if resolved != default_cmd {
        std::path::Path::new(&resolved).is_file()
    } else {
        is_on_path(default_cmd)
    }
}

#[tauri::command]
pub fn get_available_tools() -> Vec<AiToolInfo> {
    vec![
        AiToolInfo {
            id: "claude".to_string(),
            name: "Claude".to_string(),
            available: tool_available("claude", "claude"),
        },
        AiToolInfo {
            id: "codex".to_string(),
            name: "Codex".to_string(),
            available: tool_available("codex", "codex"),
        },
        AiToolInfo {
            id: "copilot".to_string(),
            name: "Copilot".to_string(),
            available: tool_available("copilot", "copilot"),
        },
        AiToolInfo {
            id: "cursor".to_string(),
            name: "Cursor".to_string(),
            available: tool_available("cursor", "cursor-agent"),
        },
    ]
}

#[tauri::command]
pub async fn list_backend_models(tool: String) -> Result<BackendModelConfig, String> {
    let ai_tool: AiTool = tool.parse().map_err(|e: String| e)?;
    let provider = create_provider(&ai_tool);
    Ok(provider.list_models().await)
}

#[tauri::command]
pub async fn send_recovery_action(
    manager: State<'_, Arc<SessionManager>>,
    session_id: String,
    action: String,
) -> Result<(), String> {
    let id = parse_session_id(&session_id)?;
    let recovery_action = match action.as_str() {
        "commit" => RecoveryAction::Commit,
        "stash" => RecoveryAction::Stash,
        "reset" => RecoveryAction::HardReset,
        "abort" => RecoveryAction::Abort,
        _ => return Err(format!("Unknown recovery action: {}", action)),
    };
    manager
        .send_recovery_action(&id, recovery_action)
        .await
        .map_err(|e| e.to_string())
}

fn parse_session_id(s: &str) -> Result<SessionId, String> {
    let uuid = uuid::Uuid::parse_str(s).map_err(|e| format!("Invalid session ID: {}", e))?;
    Ok(SessionId(uuid))
}

fn expand_tilde(path: &str) -> PathBuf {
    let expanded = if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            PathBuf::from(home).join(rest)
        } else {
            PathBuf::from(path)
        }
    } else if path == "~" {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            PathBuf::from(home)
        } else {
            PathBuf::from(path)
        }
    } else {
        PathBuf::from(path)
    };
    // Canonicalize to resolve symlinks — ensures the stored path matches
    // what getcwd() returns when the AI runs inside the worktree.
    expanded.canonicalize().unwrap_or(expanded)
}
