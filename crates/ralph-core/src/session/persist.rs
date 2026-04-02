use std::path::PathBuf;

use crate::session::state::{SessionInfo, SessionStatus};

fn persist_path() -> PathBuf {
    dirs_or_default().join("sessions.json")
}

fn dirs_or_default() -> PathBuf {
    if let Some(home) = home_dir() {
        home.join(".ralph-desktop")
    } else {
        PathBuf::from(".ralph-desktop")
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub fn save_sessions(sessions: &[SessionInfo]) -> anyhow::Result<()> {
    let dir = dirs_or_default();
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(sessions)?;
    std::fs::write(persist_path(), json)?;
    Ok(())
}

/// Load persisted sessions. Running/Stopping sessions become Aborted (crash recovery).
pub fn load_sessions() -> anyhow::Result<Vec<SessionInfo>> {
    let path = persist_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let json = std::fs::read_to_string(path)?;
    let mut sessions: Vec<SessionInfo> = serde_json::from_str(&json)?;

    // Sessions that were Running/Stopping when Ralph exited were interrupted
    for session in &mut sessions {
        match &session.status {
            SessionStatus::Running { .. } | SessionStatus::Stopping { .. } => {
                session.status = SessionStatus::Aborted {
                    ai_session_id: session.ai_session_id.clone(),
                };
            }
            _ => {}
        }
    }

    Ok(sessions)
}
