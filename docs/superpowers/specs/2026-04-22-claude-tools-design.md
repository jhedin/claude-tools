# claude-tools: zsh-integrated natural-language wrappers for Unix pipelines

**Date:** 2026-04-22
**Status:** Design

## Summary

A zsh widget that intercepts command lines containing `c`-prefixed tool names (e.g. `cfind`, `cgrep`, `cjq`) and rewrites them into real Unix commands by calling `claude -p`. The rewrite happens in place before execution, so the shell runs plain Unix tools and all normal shell features — pipes, redirects, history, job control — work unchanged.

The user types natural-language descriptions of what they want; Claude figures out the right flags. A safety checker (local allowlist + a second `claude -p` call) gates execution of anything that looks destructive or ambiguous.

## Motivation

The Unix toolbox is powerful but the flag vocabulary is vast and poorly memorable. `find`, `jq`, `awk`, `sed`, `ffmpeg`, and their kin have hundreds of options that most users look up every time. Meanwhile, pipelines composed from these tools are exactly where experienced shell users spend a lot of time.

This project lets the user describe intent in natural language at each stage of a pipeline, while preserving the Unix composability that makes shell work valuable. The executed command is real — no LLM in the hot path of data flow — so performance, correctness, and the ability to further pipe into arbitrary tools are all preserved.

## User Experience

### Happy path

The user types:

```
$ cfind . "python files modified this week" | cgrep "TODO" | sort | uniq -c
```

Hits Enter. The widget intercepts, calls `claude -p` with the full buffer, gets back a rewritten command. After a safety check, the prompt line is rewritten in place and executed:

```
$ find . -name '*.py' -mtime -7 | xargs grep -l TODO | sort | uniq -c
# finds python files modified this week containing TODO, counts per file
      3 ./src/app.py
      1 ./tests/test_auth.py
```

The dim `#` line is a one-sentence explanation from the planner, shown once between the command and its output.

### Destructive path

If the rewrite (or the user's prompt) implies something destructive, or includes patterns on the rejectlist, the widget prompts for confirmation:

```
$ find . -name '*.py' -mtime -7 -delete
# would delete all python files modified in the last week
⚠ dangerous: -delete removes files; request said nothing about deleting
Execute? [y/N]
```

Rejection returns the rewritten command as an editable buffer so the user can tweak and re-submit.

### Prefixes

- `c?` (e.g. `c?find . "..."`) — dry-run: plan and display, never execute.
- `cc` (e.g. `ccfind . "..."`) — force `[y/N]` confirm even on a `safe` verdict.

### History

After each intercepted Enter, zsh history contains two entries:

```
$ find . -name '*.py' -mtime -7 | xargs grep -l TODO | sort | uniq -c   ← most recent (Up-arrow × 1)
$ cfind . "python files modified this week" | cgrep "TODO" | sort | uniq -c   ← previous (Up-arrow × 2)
```

Up-arrow once recalls the exact command that ran, for flag tweaking. Up-arrow twice recalls the natural-language version, which replans on Enter (useful when the same intent should produce different flags in a different directory).

Both entries persist across sessions via `HISTFILE`.

## Architecture

### Components

1. **The zsh widget** (`claude-tools.zsh`, sourced from `~/.zshrc`). Binds `accept-line`. On Enter: scans `$BUFFER` for `c`-prefixed tokens; if none, passes through. Otherwise orchestrates planning, safety check, display, execution, and history.

2. **Aliases**. Auto-generated from the config's tool list. Each alias is a function that prints a helpful error if invoked outside the widget (e.g. from a script). The widget matches on the token name, not on the alias body.

3. **The planner**. A script (language TBD — see *Language Choice* below) that takes the raw buffer and `$PWD`, calls `claude -p` with a system prompt describing the rewrite task, returns JSON `{command, explanation}`.

4. **The safety checker**. Two layers:
   - **Layer 1 (local, instant):** allowlist/rejectlist check on the rewritten command. Produces a verdict of `fast-safe`, `needs-smart-read`, or `reject`.
   - **Layer 2 (`claude -p`):** runs when Layer 1 is `needs-smart-read` or `reject`. Returns JSON `{verdict: safe | suspicious | dangerous, reason}`.

5. **Cache**. Keyed on `sha256(buffer + cwd)`. Value: `{command, explanation, layer1_verdict, layer2_verdict, layer2_reason}`. Stored in `~/.cache/claude-tools/rewrites.db` (sqlite) or a flat file — implementation choice.

6. **Sidecar history log**. `~/.local/share/claude-tools/history.jsonl`. One JSON object per invocation: `{timestamp, cwd, original_buffer, rewritten, explanation, layer1_verdict, layer2_verdict, layer2_reason, executed}`.

### Data flow

```
Enter pressed
  │
  ▼
Widget scans $BUFFER for c-prefix tokens
  │
  ├─ no match ──► normal accept-line
  │
  ▼ match
Cache lookup (sha256 of buffer + cwd)
  │
  ├─ hit ──► skip to Display
  │
  ▼ miss
Planner call: claude -p → {command, explanation}
  │
  ▼
Layer 1 check on rewritten command
  │
  ├─ fast-safe ──► Display → Execute
  │
  ├─ needs-smart-read
  │      │
  │      ▼
  │   Layer 2 call: claude -p → {verdict, reason}
  │      │
  │      ├─ safe ──► Display → Execute
  │      ├─ suspicious ──► Display → [y/N] prompt
  │      └─ dangerous ──► Display → [y/N] prompt, default N
  │
  └─ reject
         │
         ▼
      Layer 2 call (for reason text only) → {verdict, reason}
         │
         ▼
      Display → [y/N] prompt (regardless of Layer 2 verdict)
```

After execution: write both history entries (natural-language then rewritten) via `print -s`, append sidecar log, store cache entry.

### Latency budget

- Cached buffer → instant (cache lookup, no LLM).
- `fast-safe`, uncached → one LLM round-trip (planner only).
- `needs-smart-read`, uncached → two LLM round-trips (planner → checker).
- `reject`, uncached → two LLM round-trips + `[y/N]` prompt.

No daemon in v1. Per-Enter startup cost of the chosen language is accepted.

## Safety Checker Design

### Layer 1: Allowlist + Rejectlist

**Allowed tools** (safe by default, any flags):
`find`, `grep`, `rg`, `ag`, `jq`, `yq`, `awk`, `gawk`, `sed` (without `-i`), `sort`, `uniq`, `cut`, `tr`, `wc`, `head`, `tail`, `ls`, `cat`, `echo`, `printf`, `column`, `paste`, `comm`, `diff`, `fold`, `rev`, `tac`, `seq`, `nl`, `od`, `xxd`, `file`, `stat`, `readlink`, `realpath`, `basename`, `dirname`, `date`, `env`, `printenv`, `which`, `type`, `command`. Git read-only subcommands: `git log`, `git diff`, `git status`, `git show`, `git blame`, `git ls-files`, `git rev-parse`.

**Write-ish but allowed** (Layer 1 permits, Layer 2 may still flag):
- `curl` / `wget` downloading to files inside `$PWD` or `/tmp` (not piping to shell).
- Output redirection `>`, `>>` to files inside `$PWD` or `/tmp`.
- `tee` to files inside `$PWD` or `/tmp`.
- `mkdir`, `touch`.
- `git commit`, `git add`, `git stash push`, `git tag`, `git branch <name>` (create), `git fetch`, `git pull`.
- `docker build`, `docker pull`.
- `npm run`, `cargo build`, `make` (without obvious `clean` targets).

**Force smart read** (Layer 1 → `needs-smart-read`, always triggers Layer 2):
- `xargs` — invocation always forces Layer 2 regardless of what it feeds.
- `tee` (any).
- Compound commands: `&&`, `;`, `||`.
- Command substitution: `$(…)`, backticks.
- Redirection to paths outside `$PWD` or `/tmp`.
- `curl -o` (any).
- Any tool not in the allowlist but also not in the rejectlist.

**Rejected patterns** (Layer 1 → `reject`, confirm always required):
- `rm`, `rmdir`, `shred`, `dd`, `truncate -s 0`, `unlink`.
- `mv` with an existing destination (would overwrite).
- `sed -i`, `perl -i`, `find ... -delete`, `find ... -exec rm`.
- `chmod`, `chown`, `chgrp`, `setfacl`.
- `sudo`, `doas`, `su`.
- `curl | sh`, `wget | sh`, or any pipe into a shell.
- `eval`, `exec`, `source`, `.` (dot-source).
- `git checkout .`, `git checkout -- <path>`, `git checkout <branch>`, `git restore`, `git reset --hard`, `git clean`, `git branch -D`, `git push --force`, `git rebase`, `git cherry-pick`, `git revert`, `git stash drop`, `git stash clear`, `git merge`.
- `docker rm`, `docker rmi`, `docker system prune`, `docker run`, `docker exec`.
- `kubectl apply`, `kubectl delete`, `kubectl exec`, `kubectl patch`.
- Package-manager state changes: `npm install`, `pip install`, `cargo install`, `apt`, `brew install`, etc.

### Layer 2: Semantic Checker

Runs when Layer 1 returns `needs-smart-read` or `reject`. Prompt shape:

> You are reviewing a shell command that was auto-generated from a natural-language request.
>
> **User's original request (the buffer they typed, including `c*` wrappers):** `{original_buffer}`
> **Rewritten command the planner produced:** `{rewritten}`
> **Current working directory:** `{pwd}`
>
> Answer with JSON: `{"verdict": "safe" | "suspicious" | "dangerous", "reason": "<one short sentence>"}`
>
> - `safe`: command matches the user's request, no surprises, read-only or clearly intentional writes.
> - `suspicious`: command does what was asked but includes unusual side effects, writes to unexpected locations, or uses rarely-correct flags.
> - `dangerous`: command could destroy data, send network traffic to unexpected hosts, or does something materially different from the user's request.

### Verdict handling

| Layer 1 | Layer 2 | Outcome |
|---|---|---|
| `fast-safe` | not run | Execute immediately |
| `needs-smart-read` | `safe` | Execute immediately |
| `needs-smart-read` | `suspicious` | `[y/N]` prompt, reason shown |
| `needs-smart-read` | `dangerous` | `[y/N]` prompt, reason shown in red, default N |
| `reject` | any | `[y/N]` prompt regardless (Layer 2 reason shown) |

## Configuration

`~/.config/claude-tools/config.toml`:

```toml
[planner]
model = "claude-sonnet-4-6"
timeout_seconds = 30

[safety]
model = "claude-sonnet-4-6"
timeout_seconds = 15

[cache]
enabled = true
path = "~/.cache/claude-tools/rewrites.db"

[aliases]
# Tools get `c`-prefixed aliases auto-generated at shell startup.
tools = ["find", "grep", "jq", "awk", "sed"]

[allowlist]
# User additions (merged with built-in allowlist).
extra_safe = []

[rejectlist]
# User additions (merged with built-in rejectlist; regex patterns).
extra_reject = []
```

Config loaded when `claude-tools.zsh` is sourced. Reload by re-sourcing.

Starter alias set: `cfind`, `cgrep`, `cjq`, `cawk`, `csed`. User extends via `[aliases].tools`.

## Error Handling

- `claude -p` fails or times out → widget shows `⚠ planner unavailable` and submits the buffer unmodified. This produces `command not found: cfind`, a clear signal that the tool failed — no silent wrong behavior.
- Planner returns malformed JSON → same fallback, error logged to `~/.local/share/claude-tools/errors.log`.
- Safety checker times out with planner having succeeded → treat as `suspicious` and show the `[y/N]` prompt. Fail closed.
- Cache read error → ignore cache, proceed to planner call.
- Cache write error → log to `errors.log`, continue execution.

## Language Choice

TBD at implementation time. Candidates:

- **Python** — fast to build, good JSON and sqlite support, 100–200ms startup cost per invocation.
- **Go** — ~5–20ms startup, clean single-binary distribution, moderate effort.
- **Rust** — fastest, steepest learning curve for a user new to the language.

No daemon in v1. Re-evaluate only if per-Enter latency feels bad in practice.

## Testing Plan

### Unit

- Layer 1 allowlist/rejectlist classification. Feed a fixture of known commands, assert `{fast-safe, needs-smart-read, reject}` verdict.
- Shell-command tokenizer (dependency of Layer 1).
- Cache read/write/hash stability.
- Buffer parser: "does this buffer contain a `c`-prefixed token we should intercept?" Handles edge cases: `c` token inside a quoted string, inside a comment, inside `$(…)`.
- History writer: given a mock zsh state, verify two-entry sequence.

### Integration

- Mock `claude -p` with a script returning canned JSON. Drive the widget with a fixture of buffers, assert final executed command and displayed messages for each verdict path.

### End-to-end smoke

- Real `claude -p`, small corpus of natural-language pipelines with known-good rewrites. Run manually; not in CI.

### Manual checklist (first working build)

- `cfind . "python files"` runs real `find`.
- `cfind ... | cgrep ...` plans as one unit.
- Crafted prompt that should yield `-delete` → confirm prompt appears.
- Same buffer twice → second is instant (cache hit).
- `c?` dry-run does not execute.
- Network disconnected → graceful fallback message.
- Up-arrow history shows both entries in correct order.

## Scope Boundaries (v1)

**In:**
- zsh interactive sessions only.
- Starter aliases: `cfind cgrep cjq cawk csed`, user-extensible via config.
- Hosted `claude -p` for both planner and checker.
- Cache keyed on `sha256(buffer + cwd)`.
- `c?` dry-run and `cc` confirm-always prefixes.
- Two history entries per intercepted Enter (natural-language + rewritten).
- JSONL sidecar history log.
- Layer 1 allowlist/rejectlist as specified above.
- Layer 2 semantic checker via `claude -p`.

**Out (explicitly):**
- Non-zsh shells. Scripts.
- Local LLMs.
- Daemon or any persistent background process.
- Polished preview UX (streaming preview, inline editing of the plan before execution).
- Context feeding beyond `buffer + pwd` (no stdin peek, no shell history snooping, no downstream-pipe awareness — the whole-buffer plan covers this need).
