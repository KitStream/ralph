use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub layout: LayoutMode,
    #[serde(default = "default_theme")]
    pub theme: ThemeMode,
    pub default_ai_tool: String,
    pub default_main_branch: String,
    pub default_tagging_enabled: bool,
    #[serde(default)]
    pub recent_project_dirs: Vec<String>,
    #[serde(default)]
    pub recent_preambles: Vec<String>,
    #[serde(default)]
    pub default_mode: String,
    #[serde(default)]
    pub default_preamble: String,
    #[serde(default = "default_tool_output_preview_lines")]
    pub tool_output_preview_lines: u32,
    /// Per-tool binary path overrides keyed by tool id
    /// ("claude", "codex", "copilot", "cursor"). Empty strings mean "use PATH".
    #[serde(default)]
    pub tool_paths: HashMap<String, String>,
}

fn default_tool_output_preview_lines() -> u32 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ThemeMode {
    Dark,
    Light,
}

fn default_theme() -> ThemeMode {
    ThemeMode::Dark
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
            theme: ThemeMode::Dark,
            default_ai_tool: "claude".to_string(),
            default_main_branch: "main".to_string(),
            default_tagging_enabled: true,
            recent_project_dirs: Vec::new(),
            recent_preambles: Vec::new(),
            default_mode: String::new(),
            default_preamble: String::new(),
            tool_output_preview_lines: 2,
            tool_paths: HashMap::new(),
        }
    }
}

/// Push the tool path overrides from settings into the ralph-core provider
/// registry so spawned backends see them.
pub fn apply_tool_path_overrides(settings: &AppSettings) {
    ralph_core::provider::set_tool_path_overrides(settings.tool_paths.clone());
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
    save_settings(&settings)?;
    apply_tool_path_overrides(&settings);
    Ok(())
}
