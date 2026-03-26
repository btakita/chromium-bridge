# chromium-bridge

Rust CLI bridging agents to Chromium-based browsers via Chrome DevTools Protocol.

## Layout

```
chromium-bridge/
├── src/
│   └── main.rs       # CLI entry point + all commands
├── Cargo.toml
├── Makefile
├── README.md
├── SPEC.md
└── CLAUDE.md
```

## Build

```bash
make check    # clippy + test
make build    # release build
```

## Conventions

- Single-binary CLI, no library crate (yet)
- All CDP communication via HTTP (`/json/*`) and WebSocket
- Default port 9222, configurable via `--port` or `CHROMIUM_BRIDGE_PORT`
- Errors go to stderr, data goes to stdout
- `--json` flag for machine-readable output on all commands
