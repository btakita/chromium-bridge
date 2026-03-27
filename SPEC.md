# chromium-bridge — Specification

## Purpose

A Rust CLI that bridges AI agents and scripts to Chromium-based browsers via the Chrome DevTools Protocol (CDP). Provides deterministic, scriptable operations for browser automation without requiring a full MCP server.

## Architecture

```
Agent / Script
    │
    ▼
chromium-bridge CLI
    │
    ▼ HTTP + WebSocket
CDP endpoint (127.0.0.1:9222)
    │
    ▼
Brave / Chrome / Chromium
```

### CDP Communication

- **HTTP API** (`/json/*` endpoints): Tab listing, version info, health checks
- **WebSocket API** (`ws://...`): Page-level commands (navigate, evaluate, screenshot)

## Commands

### `check`
Health check. Hits `/json/version`, prints browser name and protocol version. Exit 0 if responding, exit 1 if not.

### `list`
Lists open tabs. Hits `/json/list`, outputs tab title + URL. Supports `--json` for machine-readable output.

### `navigate <url>`
Opens a URL. Creates a new tab via `/json/new?<url>` or navigates the active tab via WebSocket `Page.navigate`.

### `evaluate <expression>`
Runs JavaScript in the active tab via WebSocket `Runtime.evaluate`. Prints the return value to stdout.

### `screenshot [url]`
Captures a screenshot via WebSocket `Page.captureScreenshot`. If URL is provided, navigates first. Outputs PNG to stdout or file via `--output`.

### `markdown <url>`
Navigates to URL, extracts page content, converts to clean markdown. Uses `Readability`-style extraction via injected JS.

### `setup`
Interactive setup wizard:
1. Detects installed Chromium browsers
2. Configures `--remote-debugging-port` in the appropriate flags file
3. Verifies the port is responding after browser restart

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CHROMIUM_BRIDGE_PORT` | `9222` | CDP port |
| `CHROMIUM_BRIDGE_HOST` | `127.0.0.1` | CDP host |

### CLI Flags

- `--port <N>` — override CDP port
- `--host <addr>` — override CDP host
- `--json` — machine-readable JSON output
- `--timeout <ms>` — connection timeout (default: 5000)

## Dependencies

- `tokio` — async runtime
- `reqwest` — HTTP client for CDP REST API
- `cdpkit` — type-safe CDP client (auto-generated typed bindings for all CDP domains)
- `clap` — CLI argument parsing
- `serde` / `serde_json` — JSON serialization
- `base64` — screenshot decoding
- `futures` — event stream subscription

## Error Handling

- Connection refused → "Browser not responding on {host}:{port}. Is remote debugging enabled?"
- WebSocket timeout → "Command timed out after {timeout}ms"
- Invalid JS → Forward CDP error message

## Security

- CDP port binds to localhost only — no external exposure
- No secrets handled by the CLI itself
- Screenshot output goes to local files only
