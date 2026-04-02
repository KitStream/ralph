---
name: rebase
description: Rebase the current branch onto origin/main, safely handling uncommitted work
disable-model-invocation: true
---

# Rebase onto origin/main

Rebase the current checkout onto the latest origin/main, preserving any in-progress work.

## Steps

1. **Save uncommitted work**:
   - Run `git status` to check for uncommitted changes (staged, unstaged, or untracked).
   - If there are changes: run `git stash --include-untracked` to save them.
   - Note whether a stash was created for step 5.

2. **Fetch**: Run `git fetch origin main` to get the latest remote state.

3. **Check if rebase is needed**:
   - Run `git merge-base --is-ancestor origin/main HEAD` to check if already up-to-date.
   - If origin/main is already an ancestor of HEAD, skip to step 5 (no rebase needed).

4. **Rebase**: Run `git rebase origin/main`.
   - If the rebase produces conflicts:
     - Show the conflicting files with `git diff --name-only --diff-filter=U`.
     - Ask the user how to proceed (resolve, skip, or abort).
     - Do **not** force-resolve conflicts automatically.
   - If the rebase succeeds, continue.

5. **Restore stashed work**: If changes were stashed in step 1, run `git stash pop`.
   - If the stash pop conflicts, inform the user and leave the stash applied (`git stash pop` will have left conflict markers — do not drop the stash).

6. **Verify**: Run `git log --oneline -5` and then `git status` to confirm the result.
