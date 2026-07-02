---
description: Run a read-only Claude Code review against local git state
argument-hint: '[--background] [--base <ref>] [--scope auto|working-tree|branch] [--model <id>] [--adversarial] [focus text]'
allowed-tools: Bash(${GROK_PLUGIN_ROOT}/bin/claude-companion:*), Bash(${CLAUDE_PLUGIN_ROOT}/bin/claude-companion:*), Bash(*/bin/claude-companion:*), Bash(git:*)
---

Run a Claude Code review through the companion runtime. Review-only — do not fix issues yourself.

Raw slash-command arguments: `$ARGUMENTS`

Run:

```bash
"${GROK_PLUGIN_ROOT}/bin/claude-companion" review $ARGUMENTS
```

Use `CLAUDE_PLUGIN_ROOT` if `GROK_PLUGIN_ROOT` is unset. Not every host sets either — Codex
does not inject a plugin-root env var into command Bash calls, so both may be empty there.
If so, locate this plugin's own installed `bin/claude-companion` yourself (from the
installed-plugin path your host reports, or this command file's own directory) and run that
path directly.

- Pass the user's arguments through unchanged.
- Model is unset by default (inherits the user's Claude Code default) unless `--model` is set or `CLAUDE_DELEGATE_MODEL` is configured.
- For background runs, invoke the Bash call with `run_in_background: true` when `--background` is present.
- Return the companion's stdout as-is. Do not add your own analysis.

If the companion reports Claude is not ready, tell the user to run `/claude:setup`.