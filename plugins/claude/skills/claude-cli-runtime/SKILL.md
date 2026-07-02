---
name: claude-cli-runtime
description: How to delegate tasks to the Claude Code CLI from Grok (or other compatible hosts) via the claude-companion binary. Use when forwarding coding, debugging, or review work to Claude, driving claude headlessly, or when the claude-rescue subagent or /claude:rescue command needs to invoke the companion.
---

# Claude CLI runtime

Delegate work to Claude Code through one companion binary. The companion wraps
Claude's headless `-p/--print` mode so the host agent gets a clean hand-off.

## Entry point

Always invoke the companion — never call `claude` directly from the delegation path:

```bash
"${GROK_PLUGIN_ROOT}/bin/claude-companion" <setup|task|review> [args]
```

`${GROK_PLUGIN_ROOT}` is set by Grok for installed plugins. `${CLAUDE_PLUGIN_ROOT}` is the
compatibility alias. **Not every host sets either** — Codex, for example, does not inject a
plugin-root env var into command/agent Bash calls, so both are empty there. When both are
unset, locate this plugin's own installed `bin/claude-companion` yourself (from the
installed-plugin path your host reports, e.g. via `grok plugin list` / `codex plugin list`,
or from this skill/command file's own directory) and invoke that path directly instead of
the `${...}`-templated form above.

## Default model

Delegated runs have no hardcoded model by default — the companion omits `--model` unless one is
supplied, so Claude Code falls back to the user's own configured default model. Override per
invocation:

- `--model opus` — Claude Opus
- `--model sonnet` — Claude Sonnet 5
- `--model <full-name>` — e.g. `claude-opus-4-8`

Or set `CLAUDE_DELEGATE_MODEL` in the environment to set a default globally.

## Readiness

```bash
"${GROK_PLUGIN_ROOT}/bin/claude-companion" setup
```

Reports binary, version, auth source, and the effective default model (unset unless `CLAUDE_DELEGATE_MODEL` is configured). Add `--json` for machine-readable output. Add `--probe` to run a one-line smoke test.

## Delegating a task

```bash
"${GROK_PLUGIN_ROOT}/bin/claude-companion" task [routing flags] "<task text>"
```

| Flag | Effect |
| --- | --- |
| _(default)_ | write-capable (`--permission-mode acceptEdits`) |
| `--read` | read-only plan mode; no file edits |
| `--background` | detached process; returns pid + log path |
| `--effort <level>` | Claude effort level |
| `--model <id>` | model alias or full name (default: unset, inherits Claude Code's own default) |
| `--cwd <path>` | working directory |
| `--add-dir <path>` | extra allowed directories (repeatable) |
| `--worktree [name]` | run in a fresh git worktree |
| `--resume [session-id]` | resume a specific session (`-r <session-id>`); with no value, continue the most recent session here (`-c`) |

## Review

```bash
"${GROK_PLUGIN_ROOT}/bin/claude-companion" review [--base main] [--scope branch] [focus]
```

Always read-only. Diff is embedded in the prompt so Claude does not need shell access.

## Notes

- Claude JSON uses `result` and `session_id` (not Grok's `text` / `sessionId`). The companion normalizes this in rendered output.
- Delegation sends prompts and any code Claude reads to Anthropic's backend.
- Override binary with `CLAUDE_BIN` if `claude` is not on PATH.