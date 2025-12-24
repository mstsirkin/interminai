# Git Rebase Workflow with Interminai

When rebase stops (on a conflict, edit command, etc), there are options:

## Option 1: Run rebase in a shell, use interminai for interactive parts
- Run rebase in a shell (or interminai)
- Use interminai to run an editor
- Use interminai for interactive `git add -i` / `git add -p`

## Option 2: Handle edits outside, continue inside interminai (Tested)
- Start rebase (in shell or interminai) - it stops on conflict
- Resolve conflict outside interminai using standard tools (Read, Edit, Write)
- Stage the resolved file: `git add <file>`
- Use interminai for `git rebase --continue` (which opens editor)
- Save/quit in vim to complete

**Note:** This does not require starting rebase inside interminai. However, you do NOT want to run `rebase --continue` outside of interminai since that will be interactive again!

## Test Results

Tested Option 2 successfully:
1. Created `/tmp/interminai-rebase-demo` with conflicting branches
2. Started `git rebase master` in interminai - stopped on conflict
3. Resolved conflict using Write tool (outside interminai)
4. Staged with `git add` (outside interminai)
5. Ran `git rebase --continue` in interminai with GIT_EDITOR=vim
6. Saved commit message with `:wq` in vim
7. Rebase completed successfully
