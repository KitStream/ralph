use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use ralph_core::discovery;
use ralph_core::events::{RecoveryAction, SessionEvent};
use ralph_core::provider::AiTool;
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

#[tauri::command]
pub async fn create_session(
    manager: State<'_, Arc<SessionManager>>,
    request: CreateSessionRequest,
) -> Result<String, String> {
    let ai_tool: AiTool = request
        .ai_tool
        .parse()
        .map_err(|e: String| e)?;

    let config = SessionConfig {
        project_dir: expand_tilde(&request.project_dir),
        mode: request.mode,
        prompt_file: PathBuf::from(request.prompt_file),
        branch_name: request.branch_name,
        main_branch: request.main_branch,
        preamble: request.preamble,
        tagging_enabled: request.tagging_enabled,
        ai_tool,
    };

    let id = manager.create_session(config).await;
    Ok(id.to_string())
}

#[tauri::command]
pub async fn start_session(
    manager: State<'_, Arc<SessionManager>>,
    app: AppHandle,
    session_id: String,
) -> Result<(), String> {
    let id = parse_session_id(&session_id)?;
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
    let emit = make_emit(app, manager.inner().clone());
    manager
        .resume_session(&id, emit)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_session(
    manager: State<'_, Arc<SessionManager>>,
    session_id: String,
) -> Result<(), String> {
    let id = parse_session_id(&session_id)?;
    manager.stop_session(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cancel_stop_session(
    manager: State<'_, Arc<SessionManager>>,
    session_id: String,
) -> Result<(), String> {
    let id = parse_session_id(&session_id)?;
    manager
        .cancel_stop_session(&id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn abort_session(
    manager: State<'_, Arc<SessionManager>>,
    session_id: String,
) -> Result<(), String> {
    let id = parse_session_id(&session_id)?;
    manager
        .abort_session(&id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_session(
    manager: State<'_, Arc<SessionManager>>,
    session_id: String,
) -> Result<(), String> {
    let id = parse_session_id(&session_id)?;
    manager
        .remove_session(&id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_sessions(
    manager: State<'_, Arc<SessionManager>>,
) -> Result<Vec<ralph_core::session::state::SessionInfo>, String> {
    Ok(manager.list_sessions().await)
}

#[derive(Debug, Serialize)]
pub struct AiToolInfo {
    pub id: String,
    pub name: String,
}

#[tauri::command]
pub fn get_available_tools() -> Vec<AiToolInfo> {
    vec![
        AiToolInfo {
            id: "claude".to_string(),
            name: "Claude".to_string(),
        },
        AiToolInfo {
            id: "codex".to_string(),
            name: "Codex".to_string(),
        },
        AiToolInfo {
            id: "copilot".to_string(),
            name: "Copilot".to_string(),
        },
        AiToolInfo {
            id: "cursor".to_string(),
            name: "Cursor".to_string(),
        },
    ]
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
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(home).join(rest);
        }
    } else if path == "~" {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(home);
        }
    }
    PathBuf::from(path)
}
