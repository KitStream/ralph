use std::collections::HashSet;
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// Augment the process PATH so GUI-launched apps can still find
/// user-installed CLIs (Homebrew, npm global, cargo, etc.).
///
/// On macOS, GUI launches inherit only a minimal system PATH and do not source
/// the user's login shell, so we probe `$SHELL -ilc` to recover it.
/// On Linux we skip the shell probe (desktop sessions already source login
/// files) but still merge a set of common per-user bin dirs as a safety net.
/// On Windows PATH comes from the registry and is identical for GUI and
/// terminal launches, so this function is a no-op.
pub fn augment_path_for_gui_launch() {
    if cfg!(target_os = "windows") {
        return;
    }

    let mut entries: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    let push = |entries: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, p: PathBuf| {
        if !p.as_os_str().is_empty() && seen.insert(p.clone()) {
            entries.push(p);
        }
    };

    if let Some(current) = env::var_os("PATH") {
        for p in env::split_paths(&current) {
            push(&mut entries, &mut seen, p);
        }
    }

    #[cfg(target_os = "macos")]
    for p in login_shell_path_entries() {
        push(&mut entries, &mut seen, p);
    }

    for p in common_bin_dirs() {
        push(&mut entries, &mut seen, p);
    }

    if let Ok(joined) = env::join_paths(entries) {
        env::set_var("PATH", joined);
    }
}

#[cfg(target_os = "macos")]
fn login_shell_path_entries() -> Vec<PathBuf> {
    let Some(shell) = env::var_os("SHELL") else {
        return Vec::new();
    };

    let mut cmd = Command::new(&shell);
    cmd.args(["-ilc", "printf %s \"$PATH\""]);
    cmd.env_remove("PATH");

    let output = match run_with_timeout(cmd, Duration::from_secs(3)) {
        Some(out) if out.status.success() => out,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    env::split_paths(&OsString::from(trimmed)).collect()
}

#[cfg(target_os = "macos")]
fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Option<std::process::Output> {
    use std::sync::mpsc;
    use std::thread;

    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    let (tx, rx) = mpsc::channel();
    let stdout = child.stdout.take();
    thread::spawn(move || {
        let _ = tx.send(stdout.and_then(|mut s| {
            use std::io::Read;
            let mut buf = Vec::new();
            s.read_to_end(&mut buf).ok().map(|_| buf)
        }));
    });

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait().ok()? {
            Some(status) => {
                let stdout = rx
                    .recv_timeout(Duration::from_millis(200))
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                return Some(std::process::Output {
                    status,
                    stdout,
                    stderr: Vec::new(),
                });
            }
            None => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    return None;
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn common_bin_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    if cfg!(target_os = "macos") {
        dirs.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/opt/homebrew/sbin"),
        ]);
    }

    if cfg!(target_os = "linux") {
        dirs.extend([
            PathBuf::from("/home/linuxbrew/.linuxbrew/bin"),
            PathBuf::from("/home/linuxbrew/.linuxbrew/sbin"),
            PathBuf::from("/snap/bin"),
        ]);
    }

    dirs.extend([
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/local/sbin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ]);

    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        for sub in [
            ".local/bin",
            ".cargo/bin",
            ".npm-global/bin",
            ".bun/bin",
            ".deno/bin",
            ".volta/bin",
            "bin",
            "go/bin",
            ".linuxbrew/bin",
        ] {
            dirs.push(home.join(sub));
        }
    }

    dirs
}
