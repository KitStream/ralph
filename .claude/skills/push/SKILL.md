---
name: push
description: Push current commits fast-forward onto origin/main with no branching history
disable-model-invocation: true
---

# Push to origin/main (fast-forward only)

Push the current branch's commits onto origin/main. The result must be a clean, linear history — no merge commits, no branching.

## Steps

1. **Preflight**:
   - Run `git status` to ensure the working tree is clean. If there are uncommitted changes, stop and ask the user to commit or stash first.
   - Run `git fetch origin main` to get the latest remote state.

2. **Check for branching history**:
   - Run `git log --oneline --graph origin/main..HEAD` to inspect the commits being pushed.
   - If the graph shows any branching (merge commits or non-linear history), the commits must be squashed:
     - Run `git reset --soft origin/main`.
     - Create a single commit summarizing all the changes. Use the original commit messages as context for the new message.
     - Show the new commit with `git log --oneline -1` and confirm with the user before continuing.

3. **Check if origin/main has moved ahead**:
   - Run `git merge-base --is-ancestor origin/main HEAD`.
   - If origin/main is NOT an ancestor of HEAD, origin/main has diverged. Invoke `/rebase` to rebase onto origin/main, then return to step 1.

4. **Show what will be pushed and push**:
   - Run `git log --oneline origin/main..HEAD` to list the commits.
   - Run `git diff --stat origin/main..HEAD` to show the file summary.
   - Do NOT ask for confirmation — invoking `/push` is explicit intent. Proceed directly.

5. **Push**: Run `git push origin HEAD:main`.
   - This must be a fast-forward push. Do NOT use `--force` or `--force-with-lease`.
   - If the push is rejected (another push happened between fetch and push), go back to step 1.

6. **Verify**: Run `git log --oneline -3 origin/main` and `git status` to confirm success.
