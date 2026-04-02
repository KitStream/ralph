use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub layout: LayoutMode,
    pub default_ai_tool: String,
    pub default_main_branch: String,
    pub default_tagging_enabled: bool,
    #[serde(default)]
    pub recent_project_dirs: Vec<String>,
    #[serde(default)]
    pub recent_preambles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LayoutMode {
    Sidebar,
    Tabs,
    Split,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            layout: LayoutMode::Sidebar,
            default_ai_tool: "claude".to_string(),
            default_main_branch: "main".to_string(),
            default_tagging_enabled: true,
            recent_project_dirs: Vec::new(),
            recent_preambles: Vec::new(),
        }
    }
}

fn settings_path() -> std::path::PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    home.join(".ralph-desktop").join("settings.json")
}

pub fn load_settings() -> AppSettings {
    let path = settings_path();
    if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    } else {
        AppSettings::default()
    }
}

pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_settings() -> AppSettings {
    load_settings()
}

#[tauri::command]
pub fn update_settings(settings: AppSettings) -> Result<(), String> {
    save_settings(&settings)
}
