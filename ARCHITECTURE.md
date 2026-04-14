# Ralph Architecture

## Overview

Ralph is an autonomous coding agent orchestrator. It runs AI coding tools (Claude Code, GitHub Copilot, OpenAI Codex, Cursor) in a git worktree and manages the iteration loop: prompt → AI work → commit → tag → repeat.

There are two frontends (Tauri desktop GUI, CLI) sharing one core library (`ralph-core`).

## Crate structure

```
crates/ralph-core/     Core library — providers, session runner, git ops, events
crates/ralph-cli/      CLI binary
src-tauri/             Tauri desktop app (Rust backend)
src/                   React frontend for the Tauri app
```

## Data flow

```
Frontend (React/CLI)
    ↕  SessionEvent stream (typed JSON)
Session Runner (ralph-core/session/runner.rs)
    ↕  AiOutput stream (mpsc channel)
AI Provider (ralph-core/provider/*.rs)
    ↕  subprocess stdout/stderr
AI Tool (claude, copilot, codex, cursor)
```

## Provider contract

All AI backends implement the `AiProvider` trait and communicate with the session runner exclusively through `AiOutput` variants sent over an `mpsc::UnboundedSender<AiOutput>`. The provider's job is to normalize the backend's wire format into this uniform model.

### AiOutput variants

| Variant       | Meaning |
|---------------|---------|
| `Text(String)` | Complete, displayable text block. **Not** a raw streaming delta — providers must accumulate small deltas and emit coherent text chunks. |
| `ToolUse { tool_id, tool }` | The AI invoked a tool. `tool` is a `ToolInvocation` enum normalized via `parse_tool_invocation()`. |
| `ToolResult { tool_use_id, content, is_error }` | Result of a tool execution. |
| `RateLimited { message }` | Rate limit hit; session will pause. |
| `SessionId(String)` | Backend's session ID for crash recovery resume. |
| `Finished { duration_secs, cost_usd }` | Execution complete. |
| `Error(String)` | Fatal error. |

### Provider rules

1. **Text accumulation**: Providers must buffer streaming text deltas and emit `AiOutput::Text` only when a logical block is complete (e.g., when the message ends or a tool call begins). The frontend renders each `Text` as a separate display block; tiny fragments cause broken layout.

2. **Tool normalization**: All tool invocations must go through `parse_tool_invocation()` which maps backend-specific tool names and argument shapes to the canonical `ToolInvocation` enum (`Read`, `Edit`, `Write`, `Bash`, `Glob`, `Grep`, `Other`).

3. **File paths**: Tool invocations contain absolute file paths as returned by the AI. The AI runs with the worktree directory as its cwd, so paths are resolved by the OS. Providers do not modify paths.

4. **Abort handling**: Providers must watch the `abort: watch::Receiver<bool>` and kill the subprocess promptly when signaled.

## Session runner

`session/runner.rs` drives the iteration loop:

1. Set up git worktree (`.ralph/<branch>-worktree`)
2. Run AI provider with the prompt
3. Forward `AiOutput` → `SessionEventPayload` events to the frontend
4. On completion: commit, tag, check if done or iterate
5. Handle recovery (stash, hard reset, conflict resolution)

The runner treats all providers identically — it only sees `AiOutput`. Any provider-specific behavior must be encapsulated within the provider.

## Events and frontend

The session runner emits `SessionEvent` payloads, which the frontend (React or CLI) consumes:

- `AiContent { block }` — AI text, tool use, or tool result. The `AiContentBlock` enum mirrors `AiOutput` but is serializable for persistence.
- `Housekeeping { block }` — Git operations, step progress.
- `Log { category, text }` — Plain log lines.
- `StatusChanged`, `IterationComplete`, `Finished`, etc.

Events are persisted to disk as JSONL for session replay.

## Path handling

`project_dir` is canonicalized at session creation time (resolving symlinks) so that:
- The stored path matches what `getcwd()` returns inside the worktree
- The frontend can reliably prefix-match tool paths against `{project_dir}/.ralph/{branch}-worktree` for display shortening

The frontend's `shortenPath` replaces the worktree prefix with `⌂` for compact display. This uses case-insensitive matching with backslash normalization for Windows compatibility.

## Auto-update

Both frontends notify the user when a newer release is published:

- **Desktop** uses `tauri-plugin-updater`. On startup it fetches
  `https://github.com/KitStream/ralph/releases/latest/download/latest.json`,
  verifies the bundle signature against the public key in `tauri.conf.json`,
  and prompts the user to install. `useAppUpdate` (`src/hooks/useAppUpdate.ts`)
  drives the UI; `UpdateBanner` renders the top-of-window banner; `SettingsDialog`
  exposes a manual "Check for updates" button.
- **CLI** (`crates/ralph-cli/src/update_check.rs`) does a fire-and-forget HTTPS
  request to the GitHub API with a 3 s timeout and prints an upgrade hint to
  stderr if the latest release tag is numerically greater than the compiled
  `CARGO_PKG_VERSION`. Failures are silent by design — the check must never
  affect session execution.

The release workflow (`.github/workflows/release.yml`) builds signed updater
bundles per platform (`TAURI_SIGNING_PRIVATE_KEY` secret), collects their
signatures into per-platform JSON fragments, and merges them into a single
`latest.json` attached to the GitHub Release.
