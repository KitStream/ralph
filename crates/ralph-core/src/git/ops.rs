use std::path::{Path, PathBuf};
use tokio::process::Command;

use super::retry::git_retry;

pub enum RebaseError {
    Conflict(String),
    Permanent(String),
}

pub struct GitOps {
    pub project_dir: PathBuf,
    pub worktree_dir: PathBuf,
    pub branch: String,
    pub main_branch: String,
}

impl GitOps {
    pub fn new(project_dir: &Path, branch: &str, main_branch: &str) -> Self {
        let worktree_dir = project_dir
            .join(".ralph")
            .join(format!("{}-worktree", branch));
        Self {
            project_dir: project_dir.to_path_buf(),
            worktree_dir,
            branch: branch.to_string(),
            main_branch: main_branch.to_string(),
        }
    }

    pub fn worktree_git_dir(&self) -> PathBuf {
        self.project_dir
            .join(".git")
            .join("worktrees")
            .join(format!("{}-worktree", self.branch))
    }

    pub fn has_active_rebase(&self) -> bool {
        let git_dir = self.worktree_git_dir();
        git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists()
    }

    async fn run_git(&self, dir: &Path, args: &[&str]) -> Result<String, String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .await
            .map_err(|e| format!("Failed to spawn git: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let combined = if stderr.is_empty() {
            stdout.clone()
        } else if stdout.is_empty() {
            stderr.clone()
        } else {
            format!("{}\n{}", stdout, stderr)
        };

        if output.status.success() {
            Ok(combined)
        } else {
            Err(combined)
        }
    }

    /// Run an arbitrary git command in the worktree directory.
    pub async fn run_in_worktree(&self, args: &[&str]) -> Result<String, String> {
        self.run_git(&self.worktree_dir, args).await
    }

    pub async fn ensure_branch_exists(&self) -> anyhow::Result<()> {
        let result = self
            .run_git(
                &self.project_dir,
                &[
                    "show-ref",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{}", self.branch),
                ],
            )
            .await;

        if result.is_err() {
            self.run_git(&self.project_dir, &["branch", &self.branch])
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create branch: {}", e))?;
        }
        Ok(())
    }

    pub async fn ensure_worktree(&self) -> anyhow::Result<()> {
        if self.worktree_dir.exists() {
            // Validate it's a working git worktree
            if self
                .run_git(&self.worktree_dir, &["rev-parse", "--git-dir"])
                .await
                .is_err()
            {
                tokio::fs::remove_dir_all(&self.worktree_dir).await?;
                self.run_git(&self.project_dir, &["worktree", "prune"])
                    .await
                    .ok();
                self.run_git(
                    &self.project_dir,
                    &[
                        "worktree",
                        "add",
                        self.worktree_dir.to_str().unwrap(),
                        &self.branch,
                    ],
                )
                .await
                .map_err(|e| anyhow::anyhow!("Failed to recreate worktree: {}", e))?;
            }
        } else {
            self.run_git(
                &self.project_dir,
                &[
                    "worktree",
                    "add",
                    self.worktree_dir.to_str().unwrap(),
                    &self.branch,
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create worktree: {}", e))?;
        }
        Ok(())
    }

    pub async fn checkout_branch(&self) -> anyhow::Result<()> {
        let current = self
            .run_git(&self.worktree_dir, &["symbolic-ref", "--short", "HEAD"])
            .await
            .unwrap_or_default();

        if current.trim() != self.branch {
            self.run_git(&self.worktree_dir, &["checkout", &self.branch])
                .await
                .map_err(|e| anyhow::anyhow!("Failed to checkout branch: {}", e))?;
        }
        Ok(())
    }

    pub async fn fetch_main(&self) -> anyhow::Result<String> {
        let main = self.main_branch.clone();
        let project_dir = self.project_dir.clone();
        git_retry(50, || {
            let main = main.clone();
            let project_dir = project_dir.clone();
            async move {
                let output = Command::new("git")
                    .arg("-C")
                    .arg(&project_dir)
                    .args(["fetch", "origin", &main])
                    .output()
                    .await
                    .map_err(|e| format!("Failed to spawn git: {}", e))?;

                let combined = format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );

                if output.status.success() {
                    Ok(combined)
                } else {
                    Err(combined)
                }
            }
        })
        .await
    }

    pub async fn rebase_onto_main(&self) -> Result<String, RebaseError> {
        let target = format!("origin/{}", self.main_branch);
        match self
            .run_git(&self.worktree_dir, &["rebase", &target])
            .await
        {
            Ok(output) => Ok(output),
            Err(output) => {
                if self.has_active_rebase() {
                    Err(RebaseError::Conflict(output))
                } else {
                    Err(RebaseError::Permanent(output))
                }
            }
        }
    }

    pub async fn abort_rebase(&self) -> anyhow::Result<()> {
        self.run_git(&self.worktree_dir, &["rebase", "--abort"])
            .await
            .ok();
        Ok(())
    }

    pub async fn push_branch(&self) -> anyhow::Result<String> {
        let branch = self.branch.clone();
        let worktree_dir = self.worktree_dir.clone();
        git_retry(50, || {
            let branch = branch.clone();
            let worktree_dir = worktree_dir.clone();
            async move {
                let output = Command::new("git")
                    .arg("-C")
                    .arg(&worktree_dir)
                    .args(["push", "--force-with-lease", "origin", &branch])
                    .output()
                    .await
                    .map_err(|e| format!("Failed to spawn git: {}", e))?;

                let combined = format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );

                if output.status.success() {
                    Ok(combined)
                } else {
                    Err(combined)
                }
            }
        })
        .await
    }

    pub async fn push_to_main(&self) -> anyhow::Result<String> {
        let branch = self.branch.clone();
        let main = self.main_branch.clone();
        let worktree_dir = self.worktree_dir.clone();
        git_retry(50, || {
            let branch = branch.clone();
            let main = main.clone();
            let worktree_dir = worktree_dir.clone();
            async move {
                let refspec = format!("{}:{}", branch, main);
                let output = Command::new("git")
                    .arg("-C")
                    .arg(&worktree_dir)
                    .args(["push", "origin", &refspec])
                    .output()
                    .await
                    .map_err(|e| format!("Failed to spawn git: {}", e))?;

                let combined = format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );

                if output.status.success() {
                    Ok(combined)
                } else {
                    Err(combined)
                }
            }
        })
        .await
    }

    pub async fn get_head(&self) -> anyhow::Result<String> {
        self.run_git(&self.worktree_dir, &["rev-parse", "HEAD"])
            .await
            .map(|s| s.trim().to_string())
            .map_err(|e| anyhow::anyhow!("Failed to get HEAD: {}", e))
    }

    pub async fn head_changed(&self, before: &str) -> anyhow::Result<bool> {
        let after = self.get_head().await?;
        Ok(after != before)
    }

    pub async fn verify_main_is_ancestor(&self) -> anyhow::Result<bool> {
        let target = format!("origin/{}", self.main_branch);
        self.run_git(
            &self.worktree_dir,
            &["merge-base", "--is-ancestor", &target, "HEAD"],
        )
        .await
        .map(|_| true)
        .or(Ok(false))
    }

    pub async fn diff_stat_against_main(&self) -> anyhow::Result<String> {
        let range = format!("origin/{}..HEAD", self.main_branch);
        self.run_git(&self.worktree_dir, &["diff", "--stat", &range])
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get diff stat: {}", e))
    }

    pub async fn get_latest_tag(&self) -> anyhow::Result<String> {
        let output = self
            .run_git(
                &self.project_dir,
                &["tag", "--sort=-v:refname"],
            )
            .await
            .unwrap_or_default();

        let tag = output
            .lines()
            .find(|line| {
                let l = line.trim().trim_start_matches('v');
                let parts: Vec<&str> = l.split('.').collect();
                parts.len() == 3 && parts.iter().all(|p| p.parse::<u32>().is_ok())
            })
            .unwrap_or("0.0.0");

        Ok(tag.trim().to_string())
    }

    pub fn bump_patch(tag: &str) -> String {
        let tag = tag.trim_start_matches('v');
        let parts: Vec<&str> = tag.split('.').collect();
        if parts.len() == 3 {
            let major: u32 = parts[0].parse().unwrap_or(0);
            let minor: u32 = parts[1].parse().unwrap_or(0);
            let patch: u32 = parts[2].parse().unwrap_or(0);
            format!("{}.{}.{}", major, minor, patch + 1)
        } else {
            "0.0.1".to_string()
        }
    }

    /// Remove stale `.lock` files from the worktree's git directory.
    /// These are left behind when a git process crashes or is killed.
    pub async fn remove_stale_lock_files(&self, emit_log: &impl Fn(crate::events::LogCategory, String)) {
        let git_dir = self.worktree_git_dir();
        let lock_files = ["index.lock", "HEAD.lock", "refs.lock"];
        for name in &lock_files {
            let path = git_dir.join(name);
            if path.exists() {
                emit_log(
                    crate::events::LogCategory::Warning,
                    format!("Removing stale lock file: {}", path.display()),
                );
                tokio::fs::remove_file(&path).await.ok();
            }
        }
        // Also check the main repo's lock files
        let main_git_dir = self.project_dir.join(".git");
        for name in &lock_files {
            let path = main_git_dir.join(name);
            if path.exists() {
                emit_log(
                    crate::events::LogCategory::Warning,
                    format!("Removing stale lock file: {}", path.display()),
                );
                tokio::fs::remove_file(&path).await.ok();
            }
        }
    }

    pub async fn tag_and_push(&self) -> anyhow::Result<String> {
        let max_attempts = 5;
        for attempt in 1..=max_attempts {
            // Fetch tags
            self.run_git(&self.project_dir, &["fetch", "origin", "--tags"])
                .await
                .ok();

            let latest = self.get_latest_tag().await?;
            let new_tag = Self::bump_patch(&latest);

            // Try to create and push the tag
            if self
                .run_git(&self.worktree_dir, &["tag", &new_tag])
                .await
                .is_ok()
            {
                if self
                    .run_git(&self.worktree_dir, &["push", "origin", &new_tag])
                    .await
                    .is_ok()
                {
                    return Ok(new_tag);
                }
            }

            // Tag race — clean up and retry
            self.run_git(&self.worktree_dir, &["tag", "-d", &new_tag])
                .await
                .ok();

            if attempt < max_attempts {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }

        anyhow::bail!(
            "Failed to create and push a unique tag after {} attempts",
            max_attempts
        )
    }
}
