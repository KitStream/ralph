use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

const PERMANENT_FAILURE_PATTERNS: &[&str] = &[
    "fatal: Authentication failed",
    "Permission denied",
    "repository not found",
    "could not read Username",
    "HTTP 403",
    "HTTP 404",
    "already exists",
    "failed to push some refs to",
];

fn is_permanent_failure(output: &str) -> bool {
    PERMANENT_FAILURE_PATTERNS
        .iter()
        .any(|pattern| output.contains(pattern))
}

/// Retry an async operation with exponential backoff.
/// Returns the output string on success, or an error after exhausting retries.
pub async fn git_retry<F, Fut>(max_attempts: u32, mut f: F) -> anyhow::Result<String>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    let mut delay = Duration::from_secs(2);
    let max_delay = Duration::from_secs(30);

    for attempt in 1..=max_attempts {
        match f().await {
            Ok(output) => return Ok(output),
            Err(error_output) => {
                if is_permanent_failure(&error_output) {
                    anyhow::bail!(
                        "Permanent failure detected, not retrying: {}",
                        error_output
                    );
                }
                if attempt == max_attempts {
                    anyhow::bail!(
                        "Command failed after {} attempts. Last error: {}",
                        max_attempts,
                        error_output
                    );
                }
                sleep(delay).await;
                delay = std::cmp::min(delay * 2, max_delay);
            }
        }
    }
    unreachable!()
}
