#!/usr/bin/env bash
#
# ralph.sh — Autonomous coding loop
#
# Usage: ralph.sh [OPTIONS] <mode>
#
# Modes are derived from PROMPT-<mode>.md files in the script directory and CWD.
#
# Options:
#   -b, --branch NAME    Branch name (default: "ralph-<mode>")
#   -m, --main-branch NAME  Main/common branch name (default: "main")
#   -p, --preamble TEXT   Preamble text prepended to the prompt
#   -T, --no-tag          Disable automatic semver tagging
#   -h, --help            Print this help and exit
#
# How it works:
#   1. A git worktree on the chosen branch is used for all Claude work.
#   2. Each iteration:
#      0. Check out the branch, if not already checked out
#      a. Fetch origin/main and rebase onto it (resolve conflicts via Claude).
#      b. Run Claude with the prompt file for the selected mode.
#      c. Push the branch to origin.
#      d. Fetch origin/main and rebase onto it (resolve conflicts via Claude).
#      e. Fast-forward local main to the branch (checkout main, merge --ff-only).
#      f. Tag with an incremented semver patch version.
#      g. Push main and tags to origin.
#   3. The loop exits on Ctrl+C or if a STOP file is present.
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# --- Prerequisites ---

install_hint() {
    local cmd="$1"
    case "$cmd" in
        git)
            case "$OSTYPE" in
                darwin*)  echo "  brew install git" ;;
                msys*|cygwin*|win*) echo "  winget install Git.Git" ;;
                *)        echo "  sudo apt install git  # or: sudo dnf install git" ;;
            esac
            ;;
        jq)
            case "$OSTYPE" in
                darwin*)  echo "  brew install jq" ;;
                msys*|cygwin*|win*) echo "  winget install jqlang.jq" ;;
                *)        echo "  sudo apt install jq  # or: sudo dnf install jq" ;;
            esac
            ;;
        claude)
            echo "  npm install -g @anthropic-ai/claude-code" ;;
        stty)
            case "$OSTYPE" in
                darwin*)  echo "  (included with macOS)" ;;
                msys*|cygwin*|win*) echo "  (included with Git Bash / MSYS2)" ;;
                *)        echo "  sudo apt install coreutils" ;;
            esac
            ;;
        *)  echo "  (no install hint available)" ;;
    esac
}

REQUIRED_CMDS=(git jq claude stty)
missing=()
for cmd in "${REQUIRED_CMDS[@]}"; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        missing+=("$cmd")
    fi
done
if [ ${#missing[@]} -gt 0 ]; then
    echo "ERROR: Missing required commands: ${missing[*]}" >&2
    echo "" >&2
    echo "Install instructions:" >&2
    for cmd in "${missing[@]}"; do
        echo "  $cmd:" >&2
        install_hint "$cmd" >&2
    done
    exit 1
fi

# Save terminal state for restoration on exit
SAVED_TTY=$(stty -g 2>/dev/null) || true

# --- Colors ---

if [ -t 1 ]; then
    CLR_GIT=$'\033[96m'        # bright cyan
    CLR_CLAUDE=$'\033[32m'     # green
    CLR_SCRIPT=$'\033[95m'     # bright magenta
    CLR_INTERRUPT=$'\033[1;33m' # bold yellow
    CLR_RESET=$'\033[0m'
else
    CLR_GIT='' CLR_CLAUDE='' CLR_SCRIPT='' CLR_INTERRUPT='' CLR_RESET=''
fi

log_script()    { printf '%s%s%s\n' "$CLR_SCRIPT" "$*" "$CLR_RESET"; }
log_git()       { printf '%s%s%s\n' "$CLR_GIT" "$*" "$CLR_RESET"; }
log_interrupt() { printf '%s%s%s\n' "$CLR_INTERRUPT" "$*" "$CLR_RESET"; }

# Colorize stdout/stderr of a command as git output
run_git() {
    local output exit_code=0
    output=$("$@" 2>&1) || exit_code=$?
    if [ -n "$output" ]; then
        printf '%s%s%s\n' "$CLR_GIT" "$output" "$CLR_RESET"
    fi
    return $exit_code
}

if [ -f "$SCRIPT_DIR/ralph.env" ]; then
    # shellcheck source=/dev/null
    source "$SCRIPT_DIR/ralph.env"
fi

# --- Discover available modes from PROMPT-*.md files ---
# Searches both the script directory and the current working directory.

discover_modes() {
    local seen=()
    local dirs=("$SCRIPT_DIR")
    local cwd
    cwd="$(pwd)"
    # Add CWD if it differs from SCRIPT_DIR
    if [ "$cwd" != "$SCRIPT_DIR" ]; then
        dirs+=("$cwd")
    fi
    for dir in "${dirs[@]}"; do
        for f in "$dir"/PROMPT-*.md; do
            [ -f "$f" ] || continue
            local name
            name="$(basename "$f")"
            name="${name#PROMPT-}"
            name="${name%.md}"
            # Deduplicate (script dir wins if both have the same mode)
            local already=false
            for s in "${seen[@]+"${seen[@]}"}"; do
                [ "$s" = "$name" ] && already=true && break
            done
            if [ "$already" = false ]; then
                seen+=("$name")
                printf '%s\n' "$name"
            fi
        done
    done
}

AVAILABLE_MODES=()
while IFS= read -r mode; do
    AVAILABLE_MODES+=("$mode")
done < <(discover_modes)

# --- Argument parsing ---

usage() {
    sed -n '3,15s/^# \{0,1\}//p' "$0"
    echo ""
    if [ ${#AVAILABLE_MODES[@]} -gt 0 ]; then
        echo "Available modes:"
        for m in "${AVAILABLE_MODES[@]}"; do
            echo "  $m  (PROMPT-${m}.md)"
        done
    else
        echo "No PROMPT-<mode>.md files found in $SCRIPT_DIR or $(pwd)"
    fi
    exit 0
}

is_valid_mode() {
    local candidate="$1"
    for m in "${AVAILABLE_MODES[@]}"; do
        [ "$m" = "$candidate" ] && return 0
    done
    return 1
}

BRANCH_NAME=""
MAIN_BRANCH="main"
PREAMBLE=""
TAGGING_ENABLED=true
MODE=""

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            usage
            ;;
        -b|--branch)
            BRANCH_NAME="${2:?ERROR: --branch requires an argument}"
            shift 2
            ;;
        -m|--main-branch)
            MAIN_BRANCH="${2:?ERROR: --main-branch requires an argument}"
            shift 2
            ;;
        -p|--preamble)
            PREAMBLE="${2:?ERROR: --preamble requires an argument}"
            shift 2
            ;;
        -T|--no-tag)
            TAGGING_ENABLED=false
            shift
            ;;
        -*)
            echo "ERROR: Unknown option: $1" >&2
            echo "Run '$0 --help' for usage." >&2
            exit 1
            ;;
        *)
            if is_valid_mode "$1"; then
                MODE="$1"
                shift
            else
                echo "ERROR: Unknown mode: $1" >&2
                echo "Available modes: ${AVAILABLE_MODES[*]}" >&2
                echo "Run '$0 --help' for usage." >&2
                exit 1
            fi
            ;;
    esac
done

if [ -z "$MODE" ]; then
    echo "ERROR: Mode required." >&2
    if [ ${#AVAILABLE_MODES[@]} -gt 0 ]; then
        echo "Available modes: ${AVAILABLE_MODES[*]}" >&2
    else
        echo "No PROMPT-<mode>.md files found in $SCRIPT_DIR or $(pwd)" >&2
    fi
    echo "Run '$0 --help' for usage." >&2
    exit 1
fi

if [ -z "$BRANCH_NAME" ]; then
    BRANCH_NAME="ralph-${MODE}"
fi

WORKTREE_BRANCH="$BRANCH_NAME"
WORKTREE_DIR="$SCRIPT_DIR/.ralph/${BRANCH_NAME}-worktree"
WORKTREE_GIT_DIR="$SCRIPT_DIR/.git/worktrees/${BRANCH_NAME}-worktree"
LOCK_FILE="$SCRIPT_DIR/.ralph/${BRANCH_NAME}.lock"

STOP_REQUESTED=false

# --- Stop / cleanup / locking ---

CLAUDE_PID=""

# Recursively kill a process and all its descendants
kill_tree() {
    local pid=$1
    local children
    if command -v pgrep >/dev/null 2>&1; then
        children=$(pgrep -P "$pid" 2>/dev/null) || true
    else
        # Fallback for Git Bash / environments without pgrep
        children=$(ps -o pid=,ppid= 2>/dev/null | awk -v ppid="$pid" '$2 == ppid {print $1}') || true
    fi
    for child in $children; do
        kill_tree "$child"
    done
    kill "$pid" 2>/dev/null || true
}

handle_sigint() {
    if [ "$STOP_REQUESTED" = true ]; then
        log_interrupt "Second Ctrl+C — aborting immediately."
        if [ -n "$CLAUDE_PID" ] && kill -0 "$CLAUDE_PID" 2>/dev/null; then
            kill_tree "$CLAUDE_PID"
        fi
        exit 130
    fi
    STOP_REQUESTED=true
    log_interrupt "Interrupted, will stop after current iteration. Press Ctrl+C again to abort immediately."
}

check_stop() {
    if [ "$STOP_REQUESTED" = true ] || [ -f "$SCRIPT_DIR/STOP" ]; then
        log_script "Stop requested. Exiting."
        exit 130
    fi
}

cleanup() {
    # Kill the claude pipeline if it's still running
    if [ -n "${CLAUDE_PID:-}" ] && kill -0 "$CLAUDE_PID" 2>/dev/null; then
        log_script "Killing claude pipeline (PID $CLAUDE_PID)..."
        kill_tree "$CLAUDE_PID"
        wait "$CLAUDE_PID" 2>/dev/null || true
    fi

    # Restore terminal settings saved at startup
    if [ -n "${SAVED_TTY:-}" ]; then
        stty "$SAVED_TTY" 2>/dev/null || true
    fi

    # Abort any in-progress rebase in the worktree
    if [ -d "$WORKTREE_GIT_DIR/rebase-merge" ] || [ -d "$WORKTREE_GIT_DIR/rebase-apply" ]; then
        log_script "Aborting in-progress rebase during cleanup..."
        git -C "$WORKTREE_DIR" rebase --abort 2>/dev/null || true
    fi

    release_lock
}

acquire_lock() {
    mkdir -p "$(dirname "$LOCK_FILE")"
    if [ -f "$LOCK_FILE" ]; then
        local lock_pid
        lock_pid="$(cat "$LOCK_FILE" 2>/dev/null || true)"
        if [ -n "$lock_pid" ] && kill -0 "$lock_pid" 2>/dev/null; then
            log_script "ERROR: Another ralph.sh instance is running (PID $lock_pid). Exiting." >&2
            exit 1
        fi
        log_script "WARNING: Removing stale lock from dead process (PID $lock_pid)." >&2
        rm -f "$LOCK_FILE"
    fi
    echo $$ > "$LOCK_FILE"
}

release_lock() {
    if [ -f "$LOCK_FILE" ]; then
        local lock_pid
        lock_pid="$(cat "$LOCK_FILE" 2>/dev/null || true)"
        if [ "$lock_pid" = "$$" ]; then
            rm -f "$LOCK_FILE"
        fi
    fi
}

trap 'handle_sigint' INT
trap 'cleanup' EXIT

# --- Setup ---

ensure_branch_exists() {
    if ! git -C "$SCRIPT_DIR" show-ref --verify --quiet "refs/heads/$WORKTREE_BRANCH"; then
        git -C "$SCRIPT_DIR" branch "$WORKTREE_BRANCH"
    fi
}

ensure_worktree_exists() {
    if [ -d "$WORKTREE_DIR" ]; then
        # Validate it's a working git worktree
        if ! git -C "$WORKTREE_DIR" rev-parse --git-dir >/dev/null 2>&1; then
            log_script "WARNING: Worktree at $WORKTREE_DIR is corrupt. Removing and recreating..." >&2
            rm -rf "$WORKTREE_DIR"
            git -C "$SCRIPT_DIR" worktree prune
            git -C "$SCRIPT_DIR" worktree add "$WORKTREE_DIR" "$WORKTREE_BRANCH"
        fi
    else
        log_script "Creating worktree at $WORKTREE_DIR on branch $WORKTREE_BRANCH..."
        git -C "$SCRIPT_DIR" worktree add "$WORKTREE_DIR" "$WORKTREE_BRANCH"
    fi
}

# --- Git retry with exponential backoff ---

# Patterns that indicate permanent failures (no point retrying)
readonly PERMANENT_FAILURE_PATTERNS="fatal: Authentication failed|Permission denied|repository.*not found|could not read Username|HTTP 403|HTTP 404|already exists|failed to push some refs to"

git_retry() {
    local max_attempts=50
    local delay=2
    local max_delay=30
    local attempt=1
    local output
    while [ $attempt -le $max_attempts ]; do
        if output=$("$@" 2>&1); then
            [ -n "$output" ] && log_git "$output"
            return 0
        fi
        # Check for permanent failures — no point retrying these
        if echo "$output" | grep -qE "$PERMANENT_FAILURE_PATTERNS"; then
            log_git "ERROR: Permanent failure detected, not retrying: $output" >&2
            return 1
        fi
        if [ $attempt -eq $max_attempts ]; then
            log_git "ERROR: Command failed after $max_attempts attempts: $*" >&2
            log_git "Last error output: $output" >&2
            return 1
        fi
        log_git "Attempt $attempt/$max_attempts failed (${output}), retrying in ${delay}s..." >&2
        sleep $delay
        delay=$(( delay * 2 > max_delay ? max_delay : delay * 2 ))
        attempt=$((attempt + 1))
    done
}

# --- Helpers ---

get_latest_tag() {
    git -C "$SCRIPT_DIR" tag --sort=-v:refname | grep -E '^v?[0-9]+\.[0-9]+\.[0-9]+$' | head -1 || echo "0.0.0"
}

bump_patch() {
    local tag="$1"
    tag="${tag#v}"
    local major minor patch
    IFS='.' read -r major minor patch <<< "$tag"
    echo "${major}.${minor}.$((patch + 1))"
}

resolve_conflicts() {
    local error_msg="${1:-}"
    log_script "Rebase failed — invoking Claude to resolve..."
    local prompt="A git rebase in this repo failed."
    if [ -n "$error_msg" ]; then
        prompt="$prompt

The error output was:
$error_msg"
    fi
    prompt="$prompt

Diagnose the issue from the error above. If there are merge conflicts, resolve them, stage the files, and run 'git rebase --continue'. If the error is something else (e.g. unstaged changes, dirty worktree), fix that first. Do not abort the rebase."
    (cd "$WORKTREE_DIR" && claude -p "$prompt" --dangerously-skip-permissions)
}

# --- Iteration steps (matching spec 2.0–2.g) ---

# Step 0: Check out the ralph branch, if not already checked out
checkout_ralph() {
    local current
    current="$(git -C "$WORKTREE_DIR" symbolic-ref --short HEAD 2>/dev/null || true)"
    if [ "$current" != "$WORKTREE_BRANCH" ]; then
        run_git git -C "$WORKTREE_DIR" checkout "$WORKTREE_BRANCH"
    fi
}

# Steps a/d: Fetch origin/main and rebase ralph onto it
rebase_ralph_onto_main() {
    git_retry git -C "$SCRIPT_DIR" fetch origin "$MAIN_BRANCH"
    local rebase_output
    if ! rebase_output=$(git -C "$WORKTREE_DIR" rebase "origin/$MAIN_BRANCH" 2>&1); then
        log_git "$rebase_output"
        local max_conflict_attempts=5
        local conflict_attempt=1
        resolve_conflicts "$rebase_output"
        while [ -d "$WORKTREE_GIT_DIR/rebase-merge" ] || [ -d "$WORKTREE_GIT_DIR/rebase-apply" ]; do
            conflict_attempt=$((conflict_attempt + 1))
            if [ $conflict_attempt -gt $max_conflict_attempts ]; then
                log_script "ERROR: Failed to resolve rebase conflicts after $max_conflict_attempts attempts. Aborting rebase." >&2
                git -C "$WORKTREE_DIR" rebase --abort
                return 1
            fi
            resolve_conflicts "$rebase_output"
        done

        # Verify origin/main is an ancestor of HEAD (Claude didn't secretly abort)
        if ! git -C "$WORKTREE_DIR" merge-base --is-ancestor "origin/$MAIN_BRANCH" HEAD; then
            log_script "ERROR: After rebase, origin/$MAIN_BRANCH is not an ancestor of HEAD. Rebase may have been aborted." >&2
            return 1
        fi
    fi
}

# Step b: Run Claude on the ralph branch
run_claude() {
    local head_before head_after exit_code
    head_before="$(git -C "$WORKTREE_DIR" rev-parse HEAD)"

    # Job control gives the background job its own process group,
    # so terminal SIGINT (Ctrl+C) doesn't reach claude directly
    set -m
    (cd "$WORKTREE_DIR" && claude -p "$1" --dangerously-skip-permissions \
        --output-format stream-json --verbose \
      | jq --unbuffered -r --arg clr "$CLR_CLAUDE" --arg rst "$CLR_RESET" '
          if .type == "assistant" then
            .message.content[] | select(.type == "text") | "\($clr)\(.text)\($rst)"
          elif .type == "result" then
            ("\($clr)\n--- Claude finished in \(.duration_ms / 1000)s | cost: $\(.total_cost_usd) ---\($rst)"), halt
          else empty end
        ') </dev/null &
    CLAUDE_PID=$!
    set +m
    exit_code=0
    # Wait for the pipeline to finish.  On MSYS2/Git Bash the pipeline
    # can linger after claude exits (wait blocks forever).  Use a hybrid
    # approach: try a blocking wait first, then fall back to polling.
    wait "$CLAUDE_PID" 2>/dev/null || exit_code=$?
    # If wait was interrupted by a signal (Ctrl+C) or the process lingers,
    # poll with a timeout so we don't hang forever.
    if kill -0 "$CLAUDE_PID" 2>/dev/null; then
        local poll_count=0
        while kill -0 "$CLAUDE_PID" 2>/dev/null; do
            poll_count=$((poll_count + 1))
            if [ $poll_count -ge 5 ]; then
                log_script "Pipeline still alive after ${poll_count}s — killing..."
                kill_tree "$CLAUDE_PID"
                wait "$CLAUDE_PID" 2>/dev/null || true
                break
            fi
            sleep 1
        done
        # Reap the process
        wait "$CLAUDE_PID" 2>/dev/null || exit_code=$?
    fi

    if [ $exit_code -ne 0 ]; then
        log_script "WARNING: Claude exited with code $exit_code" >&2
        return 1
    fi

    head_after="$(git -C "$WORKTREE_DIR" rev-parse HEAD)"
    if [ "$head_before" = "$head_after" ]; then
        log_script "WARNING: Claude made no commits this iteration." >&2
        return 1
    fi
}

# Step c: Push the ralph branch to origin
push_ralph() {
    git_retry git -C "$WORKTREE_DIR" push --force-with-lease origin "$WORKTREE_BRANCH"
}

# Step e: Push ralph branch to origin/main (fast-forward)
push_to_main() {
    git_retry git -C "$WORKTREE_DIR" push origin "$WORKTREE_BRANCH:$MAIN_BRANCH"
}

# Steps f+g: Tag with an incremented semver patch version and push.
# Fetch-tag-push is done atomically with retry to handle concurrent ralph instances.
tag_and_push_version() {
    local max_attempts=5
    local attempt=1
    while [ $attempt -le $max_attempts ]; do
        git -C "$SCRIPT_DIR" fetch origin --tags
        local latest new_tag
        latest=$(get_latest_tag)
        new_tag=$(bump_patch "$latest")
        if git -C "$WORKTREE_DIR" tag "$new_tag" 2>/dev/null &&
           git -C "$WORKTREE_DIR" push origin "$new_tag" 2>/dev/null; then
            echo "$new_tag"
            return 0
        fi
        # Tag already exists (another instance won the race) — clean up and retry
        git -C "$WORKTREE_DIR" tag -d "$new_tag" 2>/dev/null || true
        log_git "Tag $new_tag already exists, re-fetching and retrying ($attempt/$max_attempts)..." >&2
        attempt=$((attempt + 1))
        sleep 1
    done
    log_git "ERROR: Failed to create and push a unique tag after $max_attempts attempts" >&2
    return 1
}

# --- Post-Claude git housekeeping ---

git_housekeeping() {
    push_ralph                # Step c
    rebase_ralph_onto_main    # Step d

    # Log files being pushed to main
    local diff_files
    diff_files=$(git -C "$WORKTREE_DIR" diff --stat "origin/$MAIN_BRANCH"..HEAD 2>/dev/null || true)
    if [ -n "$diff_files" ]; then
        log_script "--- Files pushed to $MAIN_BRANCH ---"
        printf '%s%s%s\n' "$CLR_GIT" "$diff_files" "$CLR_RESET"
    fi

    push_to_main              # Step e
    if [ "$TAGGING_ENABLED" = true ]; then
        NEW_TAG=$(tag_and_push_version)    # Steps f+g
    fi
    local timestamp
    timestamp="$(date '+%Y-%m-%d %H:%M:%S')"
    if [ "$TAGGING_ENABLED" = true ]; then
        log_script "=== Iteration complete: tagged $NEW_TAG ($timestamp) ==="
    else
        log_script "=== Iteration complete ($timestamp) ==="
    fi
}

recover_git_state() {
    local error_msg="$1"
    log_script "WARNING: Git housekeeping failed. Invoking Claude for recovery..." >&2
    (cd "$WORKTREE_DIR" && claude -p \
        "The autonomous coding loop (branch: $WORKTREE_BRANCH) encountered a git housekeeping error. The $WORKTREE_BRANCH branch has new commits that need to be pushed and fast-forwarded into $MAIN_BRANCH.

The following error occurred: $error_msg

Current state:
- Worktree dir: $WORKTREE_DIR (on branch $WORKTREE_BRANCH)
- Main repo dir: $SCRIPT_DIR
- Remote: origin
- Main branch: $MAIN_BRANCH

Goal: Fix the git state so that:
1. The $WORKTREE_BRANCH branch is pushed to origin
2. The $WORKTREE_BRANCH branch is pushed to origin/$MAIN_BRANCH (fast-forward)
$([ "$TAGGING_ENABLED" = true ] && echo "3. A new semver patch tag is created (check existing tags with 'git tag --sort=-v:refname')
4. Push ONLY the new tag to origin (do not use --tags, push the specific tag by name)" || echo "")

Diagnose the issue and fix it. Use 'git -C <dir>' to operate on the correct repo." \
        --dangerously-skip-permissions) || {
        log_script "WARNING: Recovery Claude also failed. Will attempt reconciliation on next iteration." >&2
        return 1
    }
}

# --- Main ---

acquire_lock
ensure_branch_exists
ensure_worktree_exists

# Load prompt from file (search script dir first, then CWD)
PROMPT_FILE=""
if [ -f "$SCRIPT_DIR/PROMPT-${MODE}.md" ]; then
    PROMPT_FILE="$SCRIPT_DIR/PROMPT-${MODE}.md"
elif [ -f "PROMPT-${MODE}.md" ]; then
    PROMPT_FILE="$(pwd)/PROMPT-${MODE}.md"
fi
if [ -z "$PROMPT_FILE" ]; then
    echo "ERROR: Prompt file PROMPT-${MODE}.md not found in $SCRIPT_DIR or $(pwd)" >&2
    exit 1
fi
PROMPT_TEXT="$(cat "$PROMPT_FILE")"
if [ -n "$PREAMBLE" ]; then
    PROMPT_TEXT="${PREAMBLE}

${PROMPT_TEXT}"
fi

log_script "Running in worktree: $WORKTREE_DIR (mode: $MODE, branch: $WORKTREE_BRANCH)"
log_script "Press Ctrl+C to stop after the current iteration, twice to abort immediately."

while [ "$STOP_REQUESTED" = false ] && [ ! -f "$SCRIPT_DIR/STOP" ]; do
    checkout_ralph
    check_stop

    if ! rebase_ralph_onto_main; then
        log_script "WARNING: Pre-Claude rebase failed. Continuing to next iteration..." >&2
        continue
    fi
    check_stop

    if ! run_claude "$PROMPT_TEXT"; then
        log_script "WARNING: Claude failed or made no commits. Skipping housekeeping." >&2
        continue
    fi

    # After Claude has made commits, always push before exiting.
    # Do not check_stop here — unpushed commits would be lost.
    if [ "$STOP_REQUESTED" = true ] || [ -f "$SCRIPT_DIR/STOP" ]; then
        log_script "Stop requested — pushing commits before exiting..."
    fi

    # Run git housekeeping; on failure, attempt recovery via Claude.
    # Use tee + temp file so output streams to the terminal in real-time
    # (command substitution silently buffers everything, which is especially
    # problematic on Git Bash / MinTTY where stderr flushing is unreliable).
    housekeeping_log="$SCRIPT_DIR/.ralph/${BRANCH_NAME}-housekeeping.log"
    if ! git_housekeeping 2>&1 | tee "$housekeeping_log"; then
        recover_git_state "$(cat "$housekeeping_log")" || log_script "WARNING: Git housekeeping failed and recovery unsuccessful. Continuing..." >&2
    fi
    rm -f "$housekeeping_log"
done
