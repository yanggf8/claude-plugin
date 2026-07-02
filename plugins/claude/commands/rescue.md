---
description: Delegate a coding, debugging, or review task to the Claude Code CLI as a parallel worker
argument-hint: '[--read] [--background] [--effort <level>] [--model <id>] <task description>'
allowed-tools: Bash(${GROK_PLUGIN_ROOT}/bin/claude-companion:*), Bash(${CLAUDE_PLUGIN_ROOT}/bin/claude-companion:*)
---

Forward the user's request to Claude Code via the companion runtime. Treat this as a thin hand-off — do not solve the task yourself.

1. Take everything in `$ARGUMENTS` as the task description, except recognized routing flags (`--read`, `--background`, `--effort <level>`, `--model <id>`, `--cwd <path>`, `--add-dir <path>`, `--worktree`, `--resume`), which pass straight through to the companion.
2. If the user gave no routing flags, choose sensible defaults:
   - Write-capable by default. Add `--read` only if the user clearly wants review/diagnosis/research with no edits.
   - Model is unset by default (inherits the user's Claude Code default) unless `--model` is provided or `CLAUDE_DELEGATE_MODEL` is set.
   - Foreground for a small, bounded task; `--background` if it looks long-running or open-ended.
3. Run exactly one command:

```bash
"${GROK_PLUGIN_ROOT}/bin/claude-companion" task <flags> "<task description>"
```

Use `CLAUDE_PLUGIN_ROOT` if `GROK_PLUGIN_ROOT` is unset.

4. Return the companion's stdout as-is. Do not add your own analysis before or after it.

If the companion reports Claude is not installed or not authenticated, tell the user to run `/claude:setup`.