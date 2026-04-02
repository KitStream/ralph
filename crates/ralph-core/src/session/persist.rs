use std::path::PathBuf;

use crate::session::state::SessionConfig;

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

pub fn save_sessions(configs: &[SessionConfig]) -> anyhow::Result<()> {
    let dir = dirs_or_default();
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(configs)?;
    std::fs::write(persist_path(), json)?;
    Ok(())
}

pub fn load_sessions() -> anyhow::Result<Vec<SessionConfig>> {
    let path = persist_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let json = std::fs::read_to_string(path)?;
    let configs: Vec<SessionConfig> = serde_json::from_str(&json)?;
    Ok(configs)
}
