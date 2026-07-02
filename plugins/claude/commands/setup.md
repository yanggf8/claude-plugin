---
description: Check whether the local Claude Code CLI is ready to delegate to
argument-hint: '[--probe]'
allowed-tools: Bash(${GROK_PLUGIN_ROOT}/bin/claude-companion:*), Bash(${CLAUDE_PLUGIN_ROOT}/bin/claude-companion:*)
---

Run:

```bash
"${GROK_PLUGIN_ROOT}/bin/claude-companion" setup $ARGUMENTS
```

If `GROK_PLUGIN_ROOT` is unset, use `CLAUDE_PLUGIN_ROOT` instead.

Then present the result to the user:

- If it reports the binary as NOT FOUND, show the install hint from the output. Do not install unless the user asks.
- If it reports not authenticated, tell the user to run `claude` interactively once or set `ANTHROPIC_API_KEY`, then re-run this command.
- If it reports Claude is ready, confirm that `/claude:rescue` and the `claude-rescue` subagent are good to go.
- Delegated model defaults to your Claude Code default (no model flag is sent) unless overridden. Set it per task with `--model` on `/claude:rescue`, or set `CLAUDE_DELEGATE_MODEL` in the environment.

Do not run any task or delegation as part of setup — this command only checks readiness.