# chromium-bridge

Rust CLI bridging agents to Chromium-based browsers via Chrome DevTools Protocol.

## Layout

```
chromium-bridge/
├── .github/
│   └── workflows/
│       └── release.yml  # Cross-platform CI release (4 targets)
├── src/
│   └── main.rs          # CLI entry point + all commands
├── Cargo.toml
├── Makefile
├── install.sh           # curl|sh installer
├── README.md
├── SPEC.md
└── CLAUDE.md
```

## Build

```bash
make check    # clippy + test
make build    # release build
```

## Domain Ontology

Extends the existence kernel vocabulary with domain-specific terms.

| Term | chromium-bridge Context |
|------|----------------------|
| **System** | The browser instance accessible via CDP |
| **Domain** | Chrome DevTools Protocol — the bounded system of browser automation |
| **Context** | A CDP session attached to a specific browser tab |
| **Signal** | CDP events (LoadEventFired, etc.) and HTTP responses from the debug port |
| **Tool** | This CLI — bridging agent intent to browser actions |

## Conventions

- Single-binary CLI, no library crate (yet)
- All CDP communication via HTTP (`/json/*`) and WebSocket
- Default port 9222, configurable via `--port` or `CHROMIUM_BRIDGE_PORT`
- Errors go to stderr, data goes to stdout
- `--json` flag for machine-readable output on all commands
- `ingest <url>` writes corky-style markdown into a resolved `mail/conversations/` corpus, can optionally mirror that document into `mailboxes/<name>/conversations/` for project-specific search corpora, and can optionally trigger `corky sync routes` / `corky ragie push`

<!-- tsift:code-navigation v=0.1.64 -->
## Code Navigation

Keep this block self-contained for Codex/OpenCode prompt reuse. If this repository also ships current `.claude/skills/tsift/SKILL.md` or `runbooks/code-navigation.md`, use those deeper runbooks for command detail instead of expanding this block.

Run `tsift status` at session start from the owning repo root. If the task or file lives under a git submodule (for example `src/tsift/...`), switch to that submodule root first so the harness loads the narrower local instructions and repo state instead of the superproject root. If status prints a `run:` recommendation for stale or missing tsift state, run `tsift status --fix` before relying on tsift results; when the harness cannot perform write commands, ask the user to run the printed command instead. Codex projects can install a prompt-time auto-reindex hook with `tsift init --codex`; OpenCode projects can install per-project tsift command shortcuts with `tsift init --opencode`.

Use the commands listed in its `use:` output:
- `tsift --envelope source-read <file> --budget normal` — AST-symbol projection with span metadata and source-window expansion commands (prefer over cat/head for source code files)
- `tsift --envelope symbol-read <symbol> --budget normal` — token-budgeted symbol body, AST span metadata, child refs, and graph/source expansion commands
- `tsift --envelope search <query> --budget normal` — AST-aware hybrid search preview (prefer over grep/rg)
- `tsift --envelope explain <symbol> --budget normal` — callers, callees, community preview
- `tsift graph <symbol> --callers` / `--callees` — call graph navigation
- `tsift summarize <symbol>` — cached summary (only when listed in `use:`)
- `tsift workflow search` — ordered exact/search/explain/summarize/digest recipe that preserves result handles across expansions

When a search envelope includes `report.scale_guard`, run one of its `narrow_commands` before dispatching parallel agents. The guard means the original result set or corpus is broad enough that fan-out should start from a narrower cited handle, path, or exact query.

Prefer bounded digest commands over raw transcript, diff, and verbose-log reads:
- `tsift --envelope session-review <path> --next-context --budget normal` or `tsift --envelope context-pack <path> --budget normal` instead of replaying long session docs, JSONL transcripts, or agent-doc runtime logs with `cat`, `tail`, or `sed`.
- `tsift diff-digest [path]` (`--cached`, `--revision <rev>`) instead of `git diff`, `git show`, or patch-style `git log`.
- `tsift --envelope digest-runner --kind test --path . --shell-command '<test command>'` / `tsift --envelope digest-runner --kind log --path . --shell-command '<build command>'` for noisy test/build/install output, or let the rewrite/hooks create those artifact-backed envelopes for `cargo test`, `pytest`, and verbose cargo commands.
- If RTK is installed, digest-runner delegates supported generic command families through `rtk rewrite` and records the chosen compact filter in `report.filter` while preserving tsift artifact handles.
- Codex, OpenCode, and other harnesses without Claude-style `PreToolUse` hooks should run `tsift rewrite --run '<command>'` before broad `rg`/recursive grep, raw transcript/session/log reads, `git diff`/`git show`/single-patch `git log`, `cargo test`/`pytest`, and cargo build/check/clippy/install commands so the same search, session-digest, diff-digest, and digest-runner rewrites apply manually. OpenCode can install this path as `/tsift-rewrite-run` with `tsift init --opencode`.

For local verification, run `make check` before committing. After local changes, check the latest GitHub Actions CI run with `gh run list --workflow CI --limit 1` and fix any failing tests before calling the work complete.

Only read full source files when tsift results are insufficient.
<!-- /tsift:code-navigation -->
