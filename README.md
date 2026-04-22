# claude-tools

zsh-interactive natural-language wrappers for Unix pipelines, powered by `claude -p`.

You type:

```
cfind . "python files modified this week" | cgrep "TODO" | sort | uniq -c
```

The zsh widget intercepts on Enter, calls `claude -p` to rewrite the whole buffer into a real Unix pipeline, safety-checks the result, and executes:

```
find . -name '*.py' -mtime -7 | xargs grep -l TODO | sort | uniq -c
# finds python files modified this week containing TODO, counts per file
```

Both entries land in your shell history — the executed command (Up-arrow × 1) and the natural-language original (Up-arrow × 2). Pipes, redirects, and composition with plain Unix tools all work because the rewritten command is just a real shell pipeline.

## Status

v1 reference implementation (Rust) landed. See [`impl-rust/`](impl-rust/).

- Design spec: [`docs/superpowers/specs/2026-04-22-claude-tools-design.md`](docs/superpowers/specs/2026-04-22-claude-tools-design.md)
- Reference implementation: [`impl-rust/`](impl-rust/)
- Planned follow-ups: Go and Python implementations behind the same CLI contract, for comparison.

## Install

Requires `zsh`, `cargo` (Rust toolchain), and the `claude` CLI (with access to `claude -p`).

```
./install.sh
```

Then add to `~/.zshrc`:

```
export CLAUDE_TOOLS_BIN=~/.local/bin/claude-tools
source ~/.local/share/claude-tools/claude-tools.zsh
```

Open a new zsh. The `c`-prefixed aliases (default: `cfind`, `cgrep`, `cjq`, `cawk`, `csed`) now trigger the widget when you hit Enter.

## Configuration

Edit `~/.config/claude-tools/config.toml` (seeded by `install.sh`). The full schema is in [`config/config.toml.example`](config/config.toml.example). Common tweaks:

- `[aliases].tools = [...]` — add more `c`-prefixed wrappers.
- `[planner].model` / `[safety].model` — switch which Claude model plans vs. checks.
- `[cache].enabled = false` — disable caching if you're iterating on planner prompts.

Re-source the widget (or open a new shell) after editing.

## Architecture (shared CLI contract)

The widget talks to the binary through subcommands; any sibling implementation that provides the same contract slots in via `CLAUDE_TOOLS_BIN`:

- `claude-tools plan --pwd <p>` — buffer on stdin, JSON `{command, explanation}` on stdout.
- `claude-tools check --rewritten-file <f> --pwd <p>` — original buffer on stdin, JSON `{layer1, layer2, reason}` on stdout.
- `claude-tools cache {get,put} --key <sha256>` — sqlite-backed rewrite cache.
- `claude-tools history append` — JSON line on stdin; appends to `~/.local/share/claude-tools/history.jsonl`.
- `claude-tools config aliases` — prints the configured tool names, one per line.

## Troubleshooting

- `command not found: cfind` — widget didn't intercept. Check that `CLAUDE_TOOLS_BIN` is set and the script is sourced in the current shell.
- `⚠ planner unavailable` — `claude -p` failed or timed out. Check network and `~/.local/share/claude-tools/errors.log`.
- Stale rewrites after editing prompts — delete `~/.cache/claude-tools/rewrites.db` to reset.

## Concept

- **Per-stage wrappers**: `cfind`, `cgrep`, `cjq`, `cawk`, `csed` out of the box. Extensible via config.
- **Whole-buffer planning**: one `claude -p` call sees the entire pipeline, not each stage in isolation.
- **Safety**: a local allowlist/rejectlist plus a second `claude -p` call that reads the proposed command for surprises. Destructive patterns (`rm`, `sed -i`, `git checkout .`, `git reset --hard`, `sudo`, etc.) require `[y/N]` confirmation.
- **Cache**: same buffer in the same cwd replays instantly — no LLM call.
- **Modifiers**: `c?find ...` dry-runs (plan and display, don't execute). `ccfind ...` forces the confirm prompt even on `safe` verdicts.

## License

TBD.
