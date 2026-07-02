# Rust rewrite of `claude-companion` — design

**Status:** implemented and verified
**Date:** 2026-07-02

## Context

`plugins/claude/scripts/claude-companion.mjs` (541 lines, Node.js) is the runtime that drives the headless `claude` CLI on behalf of Grok (or other host agents). It was recently hardened via live testing against the real `claude` CLI: the default write-capable permission mode was changed from `dontAsk` to `acceptEdits` (proven live that `dontAsk` silently denies Write/Bash headlessly), a bogus `--cwd` flag was removed from the `claude` argv (that flag doesn't exist — `claude --help` confirms it; cwd is applied only via the subprocess's working directory), and `--resume <id>` was fixed to map to `-r <id>` with bare `--resume` falling back to `-c`. All 33 tests (29 unit + 4 live, `CLAUDE_INTEGRATION=1`-gated) currently pass.

The user's other CLIs and scripts are Rust-based, and they want this companion migrated to Rust for stack consistency — not for a performance requirement. This is a **faithful behavioral port**: same subcommands, same flags, same defaults, same output strings, no new features, no job-registry model, no output-format changes.

Two advisory passes from Codex (independent review, no repo edits) shaped this design: one on cross-platform binary distribution, one reviewing the architecture below. Both are folded in.

## Approach

### Crate layout

A cargo binary crate at `plugins/claude/companion-rs/`, modules mirroring the `.mjs` file's natural sections:

- `src/main.rs` — argv dispatch (`setup`/`task`/`review`/usage), thin
- `src/resolve.rs` — `resolve_claude_bin`, `claude_version`, `auth_state`, `require_ready`, `plugin_root`
- `src/exec.rs` — `execute_claude`: foreground capture (blocking `Command::output()`-style) and detached background spawn + logfile
- `src/task.rs` — `parse_task_args`, `build_task_args`, `render_task`
- `src/review.rs` — `parse_review_args`, `gather_diff`, `build_review_prompt`, `build_review_claude_args`, `render_review`
- `src/result.rs` — `ClaudeResult` (serde struct) + `parse_result`

Dependencies: `serde` + `serde_json` only. No `clap` (hand-rolled argv parsing — the flag set is small and the current Node parser's exact lookahead semantics must be preserved literally). No `tokio` (synchronous `std::process::Command` matches Node's blocking spawn-and-wait for foreground, and a detached spawn-and-exit for background — no async runtime needed).

### Background/detach semantics (exec.rs)

Node's `spawn(..., { detached: true }).unref()` is not equivalent to Rust's plain `Command::spawn()` — a spawned child on Linux stays in the parent's process group unless explicitly detached, which matters for signal delivery. On Linux, use `std::os::unix::process::CommandExt::pre_exec` to call `setsid()` in the child before exec, replicating Node's detach behavior. Logfile: `OpenOptions::new().create(true).append(true)`, the same file handle (or two independent opens) wired to both stdout and stderr, stdin set to `Stdio::null()`. This is Linux-only for now, consistent with the single-platform distribution scope below.

### Argv parsing (highest-risk area)

The current Node parser hand-walks `argv` by index with lookahead for optional-value flags (`--worktree [name]`, `--resume [session-id]`) — greedily consuming the next token as the value only if it doesn't start with `-`. This must be ported **literally**, not reimagined. Test matrix required (per flag with optional values):
- bare flag at end of argv
- flag immediately followed by another flag (value not consumed)
- flag followed by a value, followed by more argv (prompt text)

For required-value flags (`--model`, `--effort`, `--cwd`, `--add-dir`, review's `--base`/`--scope`) missing their value at the end of argv: Node silently produces `undefined` and the flag is effectively dropped. Rust forces an explicit choice here — **decision: treat this as a hard error** (print a clear "missing value for --model" message and exit 1), since silently dropping a flag the user typed is worse than failing loudly, and this is a new-vs-old-argv edge case unlikely to be relied upon.

### JSON parsing (result.rs)

```rust
#[derive(Deserialize)]
struct ClaudeResult {
    result: Option<String>,
    session_id: Option<String>,
    stop_reason: Option<String>,
    total_cost_usd: Option<TotalCost>, // custom: accepts JSON number OR numeric string
}
```

No `deny_unknown_fields` — Claude's JSON output may grow new fields over time; unknown fields must be silently ignored (serde's default), matching Node's tolerant property access. `total_cost_usd` needs a custom deserializer or a `serde_json::Value`-first parse step to accept either a number or a numeric string, since Node's `Number(parsed.total_cost_usd)` coerces either. Malformed/non-JSON stdout falls back to raw text passthrough, same as Node's `parseResult` returning `null` on parse failure.

### Testing strategy (hybrid, per Codex's recommendation)

- **Co-located `#[cfg(test)]`** in `resolve.rs`, `task.rs`, `review.rs`, `result.rs` for pure functions: argv parsing, argv building, JSON parsing, output rendering, `gather_diff`'s truncation logic (using fixture git repos), `resolve_claude_bin`'s fallback order (mocked env/paths). Ports all 29 existing unit tests.
- **`tests/cli.rs` integration tests** against the *compiled binary*: usage banner, `setup --json` shape, exit codes — plus the 4 existing live `CLAUDE_INTEGRATION=1`-gated tests (auth check, `--read` smoke, the critical default-write-lands-on-disk proof, `--background` logfile completion proof). `execute_claude`'s subprocess-spawning logic is deliberately *not* unit-tested with real `claude` calls — it's covered by these integration/live tests instead.

### Distribution

Per Codex's advisory: ship a pre-built binary (no cargo/rustc required by end users), single platform for now (Linux x86_64 — the dev host), routed through a small launcher so adding platforms later is additive:

- `plugins/claude/bin/linux-x64/claude-companion` — committed compiled binary
- `plugins/claude/bin/claude-companion` — shell launcher: detects OS/arch, execs the matching binary, or prints a clear "unsupported platform, build from source in companion-rs/" error otherwise
- All `.md` command files, the `claude-rescue.md` agent, and `claude-cli-runtime/SKILL.md` change their invocation from `node "${GROK_PLUGIN_ROOT}/scripts/claude-companion.mjs" ...` to `"${GROK_PLUGIN_ROOT}/bin/claude-companion" ...` (full grep sweep for `claude-companion.mjs` required before considering the migration complete — this was a gap in the first documentation-only pass over this repo).

No CI build matrix yet, no platform-specific git branches, no build-on-first-run — all rejected per Codex's advisory as premature for a personal, unpublished-at-scale plugin. Migrating to multi-platform CI later only touches the launcher, not the `.md` files or the Rust source.

### Non-goals

- No behavior changes beyond the language port (already-fixed `acceptEdits`/`--cwd`/`--resume` behavior carries over as-is, not re-litigated).
- No job-registry/status/result/cancel model (that's the `codex` plugin's heavier design, explicitly out of scope per the original plan doc).
- No macOS/Windows binaries yet.
- The old `.mjs` file and `tests/companion.test.mjs` are removed once the Rust binary is verified at parity (not kept as a permanent dual-implementation).

## Critical files

- New: `plugins/claude/companion-rs/{Cargo.toml, src/*.rs, tests/cli.rs}`
- New: `plugins/claude/bin/linux-x64/claude-companion` (build output, committed), `plugins/claude/bin/claude-companion` (launcher script)
- Modify: `plugins/claude/commands/{setup,rescue,review}.md`, `plugins/claude/agents/claude-rescue.md`, `plugins/claude/skills/claude-cli-runtime/SKILL.md`, `README.md` — invocation path only (README also has a direct `node .../claude-companion.mjs setup --json` example and a file-purpose table row to update)
- Remove (once parity verified): `plugins/claude/scripts/claude-companion.mjs`, `tests/companion.test.mjs`, and the now-unused `package.json`/npm scripts (per user's stated preference to drop npm from this repo once Rust covers the same purpose)

## Verification

1. `cargo test` (in `companion-rs/`) — all ported unit tests pass.
2. `CLAUDE_INTEGRATION=1 cargo test` — live smoke tests pass against the real `claude` CLI, same bar as the Node version's final verified state (33/33 equivalent).
3. Manual diff of rendered output strings between the old `.mjs` and new binary for the same inputs (`setup`, `task --read "..."`, `task` default write, `review`) — byte-identical output format.
4. `grok plugin validate plugins/claude` still passes after the `.md`/binary changes.
5. Full grep sweep (`grep -rn "claude-companion.mjs"`) returns zero hits outside the archived plan doc under `docs/plans/`.

## Implementation record

Built via a Codex (writes tests, RED) → Grok (implements, GREEN) → Claude (independently re-verifies) pipeline, one module at a time, matching the division of labor described in a referenced dev.to writeup on multi-agent AI development. Final state: **71 tests** (62 unit + 9 integration, up from the originally-scoped 33 in the Node version — `main.rs` dispatch alone added 9 CLI-level integration tests not present before), all passing, including the 4 live `CLAUDE_INTEGRATION=1` tests against the real `claude` CLI (auth check, read-only smoke, the critical disk-write proof, and the background/logfile-completion proof).

Independent verification caught three real defects that a purely test-passing view would have missed:

1. **Cross-module test race.** `resolve.rs`'s tests temporarily clear the process-global `PATH` env var to test the `which claude` fallback path; this raced against `review.rs`'s tests, which spawn real `git` subprocesses relying on `PATH`. Reproduced consistently (10-12 failures on every unscoped `cargo test` run, 0 failures when run module-scoped) — a bug the workflow's own per-module verify agents missed because they never ran the full unscoped suite together. Fixed with a crate-wide `test_support::env_lock()` shared by both modules.
2. **Vacuous test pass after an agent timeout.** Grok's `exec.rs` implementation run was killed by a 10-minute timeout mid-work and silently lost the `#[cfg(test)] mod tests` block Codex had written, while still producing a plausible-looking implementation. `cargo test exec` "passed" only because zero tests were collected, not because real tests succeeded. Caught, and the tests were regenerated directly against the (already-correct) existing implementation, including a background-detach test whose timing assertion (`elapsed < 1s` against a 2s child sleep) can only pass under genuine process detachment.
3. **Byte-parity gaps found by a final Node-vs-Rust side-by-side diff.** `setup --json`'s `probeOk` field was omitted by serde's `skip_serializing_if` instead of serialized as `null` (Node always includes the key); the usage banner's continuation-line indentation had drifted from the Rust string literal's `\`-continuation whitespace handling. Both fixed and re-diffed to confirm parity (matching field names now, and internally consistent indentation given the intentional `node claude-companion.mjs` → `claude-companion` prefix shortening).

Distribution and cleanup landed as designed: `plugins/claude/bin/linux-x64/claude-companion` (committed release binary) + `plugins/claude/bin/claude-companion` (platform-detecting launcher), all `.md`/agent/skill/README invocation surfaces migrated off the Node script, and `plugins/claude/scripts/claude-companion.mjs`, `tests/companion.test.mjs`, and `package.json` deleted — this repo no longer has any Node/npm dependency.
