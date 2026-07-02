# claude-plugin

A lightweight [Grok](https://x.ai/cli) plugin that delegates coding, debugging, and review tasks to the [Claude Code](https://claude.com/claude-code) CLI — the reverse of `grok-plugin-claude-code` / `codex-plugin-cc`.

Works with any host that supports the Grok/Claude plugin component layout (Grok CLI today; Claude Code-compatible paths are included for portability).

## What you get

| Component | What it does |
| --- | --- |
| `claude-rescue` subagent | Thin forwarder. Hand it a substantial task and it delegates to Claude Code. |
| `/claude:rescue <task>` | One-shot delegation with optional routing flags. |
| `/claude:review` | Read-only code review against git state. |
| `/claude:setup` | Verifies Claude CLI install and auth. |
| `claude-companion` | Runtime wrapping `claude -p --output-format json`. Rust binary, invoked via a platform-detecting launcher. |

**Default model:** unset — inherits your Claude Code default model. Override with `--model` per task or `CLAUDE_DELEGATE_MODEL` in the environment.

## Requirements

- Claude Code CLI on `PATH` (or `CLAUDE_BIN` override)
- Auth: interactive `claude` login **or** `ANTHROPIC_API_KEY`
- Linux x86_64 (the only platform with a pre-built companion binary today; other platforms can build from source, see `plugins/claude/companion-rs/`)

## Install

### From a local checkout

```bash
grok plugin install /home/yanggf/b/claude-plugin/plugins/claude --trust
```

### From git (after publishing)

```bash
grok plugin marketplace add yanggf/claude-plugin
grok plugin install claude@claude-plugin --trust
```

Then:

```
/claude:setup
```

## Usage

```text
/claude:rescue fix the failing auth test in apps/api
/claude:rescue --read why is startup slow on cold boot?
/claude:rescue --background --model opus refactor the camera rig
/claude:review --scope branch --base main security regressions
```

### Routing flags

| Flag | Effect |
| --- | --- |
| _(default)_ | write-capable — Claude may edit files |
| `--read` | read-only (plan mode) |
| `--background` | detached; returns pid + log path |
| `--model <id>` | `opus`, `sonnet`, or full model name (default: unset, uses your Claude Code default) |
| `--effort <level>` | Claude effort level |
| `--cwd <path>` | working directory |
| `--resume [session-id]` | resume a specific session by id, or the most recent session here if no id is given |

### Environment

| Variable | Purpose |
| --- | --- |
| `CLAUDE_DELEGATE_MODEL` | Default model alias (unset by default) |
| `CLAUDE_BIN` | Path to `claude` binary |

## Validate

```bash
grok plugin validate /home/yanggf/b/claude-plugin/plugins/claude
/home/yanggf/b/claude-plugin/plugins/claude/bin/claude-companion setup --json
```

## License

MIT