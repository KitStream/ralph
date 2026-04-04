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

## Building from source

```bash
# CLI only
cargo build --release -p ralph-cli

# Desktop app
npm install
npm run tauri build
```
