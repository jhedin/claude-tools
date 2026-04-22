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

Design complete. Implementation in progress.

- Design spec: [`docs/superpowers/specs/2026-04-22-claude-tools-design.md`](docs/superpowers/specs/2026-04-22-claude-tools-design.md)
- Reference implementation: Rust (coming)
- Planned follow-ups: Go and Python implementations behind the same CLI contract, for comparison.

## Concept

- **Per-stage wrappers**: `cfind`, `cgrep`, `cjq`, `cawk`, `csed` out of the box. Extensible via config.
- **Whole-buffer planning**: one `claude -p` call sees the entire pipeline, not each stage in isolation.
- **Safety**: a local allowlist/rejectlist plus a second `claude -p` call that reads the proposed command for surprises. Destructive patterns (`rm`, `sed -i`, `git checkout .`, `git reset --hard`, `sudo`, etc.) require `[y/N]` confirmation.
- **Cache**: same buffer in the same cwd replays instantly — no LLM call.
- **Modifiers**: `c?find ...` dry-runs (plan and display, don't execute). `ccfind ...` forces the confirm prompt even on `safe` verdicts.

## License

TBD.
