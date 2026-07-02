# claude-plugin

A lightweight plugin that delegates coding, debugging, and review tasks to the [Claude Code](https://claude.com/claude-code) CLI — the reverse of `grok-plugin-claude-code` / `codex-plugin-cc`.

Installable into both **Grok** and **Codex** — the repo ships a manifest for each host's own plugin system (`.grok-plugin/` and `.agents/plugins/` + `.codex-plugin/`), pointing at the same shared `commands/`, `agents/`, `skills/`, and `bin/claude-companion` components.

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

### Grok

```bash
grok plugin marketplace add yanggf8/claude-plugin
grok plugin install claude@yanggf8/claude-plugin --trust
```

Or from a local checkout: `grok plugin install /path/to/claude-plugin/plugins/claude --trust`

Then:

```
/claude:setup
```

### Codex

```bash
codex plugin marketplace add yanggf8/claude-plugin
codex plugin add claude@claude-plugin
```

Or from a local checkout: `codex plugin marketplace add /path/to/claude-plugin`

Then run `/claude:setup` from within Codex the same way.

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

Run from inside your local checkout of this repo:

```bash
grok plugin validate ./plugins/claude
./plugins/claude/bin/claude-companion setup --json
```

Codex doesn't have its own `codex plugin validate` command; validate the manifest with the
scripted checker Codex's plugin-creator skill ships (path may differ per Codex install —
look under `~/.codex/skills/.system/plugin-creator/scripts/validate_plugin.py`):

```bash
python3 ~/.codex/skills/.system/plugin-creator/scripts/validate_plugin.py ./plugins/claude
```

After installing into either host, confirm the binary itself runs from its actual installed
location (the path `grok plugin list` / `codex plugin list` reports, not this repo's checkout):

```bash
<installed-plugin-path>/bin/claude-companion setup --json
```

## License

MIT