use std::time::Duration;

use colored::Colorize;
use serde::Deserialize;

const REPO: &str = "KitStream/ralph";
const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/KitStream/ralph/releases/latest";
const USER_AGENT: &str = concat!("ralph-cli/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
}

/// Check GitHub for a newer release. Prints an upgrade hint on stderr when
/// found. Any network or parse failure is swallowed — a version check must
/// never block normal CLI usage.
pub async fn check_and_notify(current: &str) {
    let latest = match fetch_latest().await {
        Ok(r) => r,
        Err(_) => return,
    };
    let Some(latest_version) = strip_release_prefix(&latest.tag_name) else {
        return;
    };
    if is_newer(latest_version, current) {
        print_upgrade_hint(current, latest_version, &latest.html_url);
    }
}

async fn fetch_latest() -> Result<Release, reqwest::Error> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(3))
        .build()?;
    client
        .get(LATEST_RELEASE_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json::<Release>()
        .await
}

/// Release tags are `release-X.Y.Z`; releases may also have a `v` prefix in
/// some projects — accept either, strip, and hand back the bare version.
fn strip_release_prefix(tag: &str) -> Option<&str> {
    if let Some(rest) = tag.strip_prefix("release-") {
        Some(rest)
    } else {
        tag.strip_prefix('v').or(Some(tag))
    }
}

/// Compare dotted numeric versions. Pre-release suffixes (after `-`) are
/// ignored for ordering; we only care about the numeric core. Returns false
/// on any parse failure — better to stay quiet than to nag on garbage.
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Option<Vec<u64>> {
        let core = s.split('-').next().unwrap_or(s).trim_start_matches('v');
        core.split('.')
            .map(|p| p.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()
    };
    match (parse(latest), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn print_upgrade_hint(current: &str, latest: &str, url: &str) {
    eprintln!(
        "{}",
        format!("A new ralph release is available: v{latest} (current v{current})")
            .yellow()
            .bold()
    );
    eprintln!("  {}", url.dimmed());
    eprintln!(
        "  {}",
        format!(
            "Upgrade: download the binary for your platform from the link above, \n          or `cargo install --git https://github.com/{REPO} ralph-cli`."
        )
        .dimmed()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_release_prefix_handles_release_tag() {
        assert_eq!(strip_release_prefix("release-1.2.3"), Some("1.2.3"));
    }

    #[test]
    fn strip_release_prefix_handles_v_tag() {
        assert_eq!(strip_release_prefix("v1.2.3"), Some("1.2.3"));
    }

    #[test]
    fn strip_release_prefix_passes_bare_version() {
        assert_eq!(strip_release_prefix("1.2.3"), Some("1.2.3"));
    }

    #[test]
    fn is_newer_detects_higher_patch() {
        assert!(is_newer("1.2.4", "1.2.3"));
    }

    #[test]
    fn is_newer_rejects_same() {
        assert!(!is_newer("1.2.3", "1.2.3"));
    }

    #[test]
    fn is_newer_rejects_lower() {
        assert!(!is_newer("1.2.2", "1.2.3"));
    }

    #[test]
    fn is_newer_rejects_garbage() {
        assert!(!is_newer("not-a-version", "1.2.3"));
    }

    #[test]
    fn is_newer_ignores_prerelease_suffix() {
        assert!(!is_newer("1.2.3-rc1", "1.2.3"));
    }
}
