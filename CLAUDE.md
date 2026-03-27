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

Extends the base ontology from `~/.claude/philosophy/src/`.

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
