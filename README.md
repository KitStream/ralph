# Ralph

Ralph is an implementation of the [Ralph Wiggum technique](https://ghuntley.com/ralph/) — an autonomous, iterative coding loop that runs AI coding agents continuously until your tasks are done.

The core insight is simple: **iteration beats perfection**. Ralph repeatedly feeds an AI agent a prompt, lets it work in an isolated git worktree, commits and pushes the results, then starts the next iteration. Progress lives in your files and git history, not in the LLM's context window. Given enough iterations, modern LLMs reliably converge on correct solutions.

Ralph comes as both a **CLI tool** and a **desktop GUI** (built with Tauri + React).

## Prerequisites

- **Git** installed and available in your PATH
- **At least one supported AI backend CLI** installed:
  - [Claude Code](https://docs.anthropic.com/en/docs/claude-code) (`claude`)
  - [GitHub Copilot CLI](https://github.com/github/copilot-cli) (`copilot`)
  - [Cursor](https://www.cursor.com/) (`cursor`)
  - [Codex](https://github.com/openai/codex) (`codex`)
- **A git-enabled project** with one or more `PROMPT-<mode>.md` files in the project root
- **Rust toolchain** (to build from source)
- **Node.js 16+** (only needed for the desktop GUI)

## Prompt files

Ralph discovers what to do by scanning your project root for files matching the pattern `PROMPT-<mode>.md`. Each file defines a *mode* — a set of instructions the AI agent receives on every iteration.

For example, a file named `PROMPT-refactor.md` creates a mode called `refactor`. Write your task description, acceptance criteria, and any constraints in this file. The AI reads it fresh on each iteration and uses git history to understand what has already been done.

## Usage

### CLI

```bash
# Run the "refactor" mode (reads PROMPT-refactor.md)
ralph refactor

# Specify a custom branch name
ralph refactor --branch my-refactor-branch

# Use a different AI backend and model
ralph refactor --backend codex --model o3

# Disable automatic semver tagging
ralph refactor --no-tag

# Add a preamble to the prompt
ralph refactor --preamble "Focus on the src/api directory only"
```

**Options:**

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--branch` | `-b` | `ralph-<mode>` | Branch name for the worktree |
| `--main-branch` | `-m` | `main` | Main/trunk branch to rebase from |
| `--backend` | `-B` | `claude` | AI backend (`claude`, `copilot`, `cursor`, `codex`) |
| `--model` | `-m` | *(backend default)* | Model override (e.g. `sonnet`, `opus`, `o3`) |
| `--preamble` | `-p` | *(empty)* | Text prepended to the prompt |
| `--no-tag` | `-T` | `false` | Disable automatic semver tagging |

Press **Ctrl+C** once to stop gracefully after the current iteration. Press it twice to abort immediately.

### Desktop GUI

```bash
# Development mode
npm run tauri dev

# Production build
npm run tauri build
```

The desktop app lets you run multiple sessions concurrently, monitor live AI output, and manage sessions with start/stop/resume controls.

## How it works

Each iteration follows this loop:

1. **Checkout** — create or reuse an isolated git worktree for the session branch
2. **Rebase** — pull latest changes from the main branch
3. **Run AI** — invoke the AI backend with your prompt file
4. **Push** — commit and push the AI's changes
5. **Tag** — optionally apply a semver tag
6. **Repeat** — start the next iteration

Ralph handles crash recovery, rate limiting (automatic pause/resume), and git conflicts (stash and retry) so the loop can run unattended for hours.

## Updates

### Desktop

The desktop app checks GitHub Releases for a new version on startup. When one
is available, a banner at the top of the window offers **Install and restart**.
You can also trigger a check manually from **Settings → About → Check for
updates**. The current version is shown in the window header and in Settings.

Updates are cryptographically signed with a release-time Ed25519 key; the app
will refuse to install a bundle whose signature doesn't match the public key
baked into `tauri.conf.json` (`plugins.updater.pubkey`).

### CLI

`ralph` runs a quick (3-second timeout) check against GitHub's releases API at
startup. If a newer release is available it prints a one-line notice with the
release URL and upgrade instructions — the running session is never blocked or
slowed down by the check.

To upgrade, download the binary for your platform from the release page or
reinstall via:

```bash
cargo install --git https://github.com/KitStream/ralph ralph-cli
```

### Maintainer: updater signing key

The desktop updater requires an Ed25519 keypair. Generate it once with:

```bash
npx tauri signer generate -w ~/.tauri/ralph-updater.key
```

- Put the **public key** into `src-tauri/tauri.conf.json` under
  `plugins.updater.pubkey`.
- Add the **private key** and its passphrase to the repo's GitHub Actions
  secrets as `TAURI_SIGNING_PRIVATE_KEY` and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

Losing the private key is unrecoverable — existing installs can no longer be
upgraded through the in-app updater and would need a fresh install.

### Maintainer: cutting a release

1. Bump the version in **all four** files (they must match):
   - `src-tauri/Cargo.toml` (canonical source)
   - `crates/ralph-cli/Cargo.toml`
   - `src-tauri/tauri.conf.json`
   - `package.json`
2. Commit and push to `main`. The release workflow will:
   - verify the four version strings agree (fail CI otherwise),
   - tag the commit as `release-X.Y.Z` if that tag doesn't already exist,
   - build and sign the CLI + desktop bundles,
   - publish them + `latest.json` to a GitHub Release.

Pushing a `release-X.Y.Z` tag by hand also works and skips the version-change
detection — useful for re-running a release.

## Building from source

```bash
# CLI only
cargo build --release -p ralph-cli

# Desktop app
npm install
npm run tauri build
```
