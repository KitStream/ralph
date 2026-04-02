use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ModeInfo {
    pub name: String,
    pub prompt_file: PathBuf,
    pub preview: String,
}

/// Discover available modes from PROMPT-*.md files.
/// Searches the given directory (and optionally a second directory).
/// Returns deduplicated modes (first directory wins on conflicts).
pub fn discover_modes(dirs: &[&Path]) -> Vec<ModeInfo> {
    let mut modes: Vec<ModeInfo> = Vec::new();
    let mut seen_names: Vec<String> = Vec::new();

    for dir in dirs {
        let pattern = dir.join("PROMPT-*.md");
        let pattern_str = pattern.to_string_lossy().to_string();

        if let Ok(entries) = glob::glob(&pattern_str) {
            for entry in entries.flatten() {
                let filename = entry
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");

                if let Some(name) = filename
                    .strip_prefix("PROMPT-")
                    .and_then(|s| s.strip_suffix(".md"))
                {
                    if seen_names.contains(&name.to_string()) {
                        continue;
                    }

                    let preview = std::fs::read_to_string(&entry)
                        .unwrap_or_default()
                        .lines()
                        .take(5)
                        .collect::<Vec<_>>()
                        .join("\n");

                    seen_names.push(name.to_string());
                    modes.push(ModeInfo {
                        name: name.to_string(),
                        prompt_file: entry,
                        preview,
                    });
                }
            }
        }
    }

    modes
}

/// Load the full prompt text from a mode's prompt file, optionally prepending a preamble.
pub fn load_prompt(prompt_file: &Path, preamble: &str) -> anyhow::Result<String> {
    let content = std::fs::read_to_string(prompt_file)
        .map_err(|e| anyhow::anyhow!("Failed to read prompt file {:?}: {}", prompt_file, e))?;

    if preamble.is_empty() {
        Ok(content)
    } else {
        Ok(format!("{}\n\n{}", preamble, content))
    }
}
