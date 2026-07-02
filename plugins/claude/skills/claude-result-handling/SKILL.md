---
name: claude-result-handling
description: Internal guidance for presenting claude-companion output back to the user without rewriting or summarizing it.
user-invocable: false
---

# Claude result handling

When `/claude:rescue`, `/claude:review`, or the `claude-rescue` subagent returns companion stdout:

1. Present it **verbatim** — the `=== claude delegate result ===` / `=== claude review ===` block is the deliverable.
2. Do not paraphrase findings, add a preface, or append your own analysis unless the user asks for a follow-up **after** showing the raw output.
3. If setup failed, surface the companion's install/auth hints and point to `/claude:setup`.
4. For background runs, the companion prints a log path — tell the user to `tail -f` that file; do not poll yourself unless asked.
5. Session continuation: when output includes `session: <uuid>`, mention `claude -c` in the same directory for follow-ups.