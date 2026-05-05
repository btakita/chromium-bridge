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

### `click <selector>`
Clicks an element by CSS selector. Resolves the element via `DOM.querySelector`, computes the center coordinates from the content quad via `DOM.getBoxModel`, and dispatches real mouse events (mouseMoved, mousePressed, mouseReleased) at those coordinates.

### `type <selector> <text>`
Types text into an element. Focuses the element via CSS selector, then sends text via CDP `Input.insertText`. Double-newlines (`\n\n`) are converted to Shift+Enter keypresses for paragraph breaks in contenteditable fields (LinkedIn, Gmail, Slack, Messenger).

### `select-tab <pattern>`
Activates a browser tab by bringing it to the foreground. Accepts a numeric index or substring pattern matching against URL or title. Uses HTTP `GET /json/activate/{tab_id}`.

### `wait <selector>`
Polls for a CSS selector to appear in the DOM. Default timeout: 10 seconds (`--wait-timeout <ms>`). Polls every 250ms.

### `snapshot`
Dumps the page accessibility tree via CDP `Accessibility.getFullAXTree`. Human-readable output shows a tree of visible nodes with stable refs (`ref=dom:<backendDOMNodeId>` when available, otherwise `ref=ax:<nodeId>`). `--json` returns a normalized snapshot object with `ref`, `parentRef`, and `childRefs`; `--raw` preserves the original AXNode protocol array for low-level debugging. Supports `--depth <n>` to limit tree depth.

### `network list`
Captures network traffic for a page load and prints a request summary. If `--url <url>` is provided, the command navigates there first; otherwise it reloads the target tab. Uses `Network.requestWillBeSent`, `Network.responseReceived`, `Network.loadingFinished`, and `Network.loadingFailed` to build a per-request view. `--capture-timeout <ms>` sets the hard stop, and `--idle-ms <ms>` ends capture after the page load goes idle.

### `network inspect <matcher>`
Captures network traffic for a page load, selects a single request by exact request id or URL substring, and prints request/response details. Response bodies are fetched via `Network.getResponseBody`; request bodies use `Network.getRequestPostData` when available. Text-like responses are decoded to UTF-8, and binary payloads stay base64.

### `ingest <url>`
Navigates to a page, extracts markdown with the same DOM walker as `markdown`, and writes a corky-style conversation document into the resolved `mail/conversations/` corpus. By default the corky data dir follows corky's own resolution order (`.corky.toml` / `corky.toml` in cwd, `mail/` in cwd, `CORKY_DATA`, then `~/Documents/mail`). `--title` overrides the subject, `--slug` overrides the filename, `--labels` / `--accounts` populate corky metadata, `--route` runs `corky sync routes`, and `--ragie-push [--ragie-full]` shells out to corky's Ragie backend after the write.

### `state save <name>`
Captures repeatable browser state for the target tab. Saves cookies applicable to the current page plus `localStorage` and `sessionStorage` for the current origin into a JSON snapshot. Named snapshots default to `~/.config/chromium-bridge/states/<name>.json`; `--path` can override the file location.

### `state load <name>`
Restores a previously-saved JSON snapshot. Applies cookies first, then navigates the target tab to each saved origin URL, restores `localStorage` and `sessionStorage`, and reloads so apps bootstrap with the restored state.

### `state list`
Lists named state snapshots in the local state directory. Supports `--json`.

### `skill install`
Installs the bundled SKILL.md to the project's `.claude/skills/chromium-bridge/` directory. Detects the project root via git superproject or toplevel. The SKILL.md is embedded at build time via `include_str!()`.

### `setup`
Interactive setup wizard:
1. Detects installed Chromium browsers (Brave, Chrome, Chromium)
2. Checks for `--remote-debugging-port` in the appropriate flags file
3. Verifies the port is responding

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CHROMIUM_BRIDGE_PORT` | `9222` | CDP port |
| `CHROMIUM_BRIDGE_HOST` | `127.0.0.1` | CDP host |
| `CHROMIUM_BRIDGE_STATE_DIR` | `~/.config/chromium-bridge/states` | Directory for named state snapshots |

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

## Distribution

- `cargo install chromium-bridge` installs from crates.io
- `curl -sSf https://raw.githubusercontent.com/btakita/chromium-bridge/main/install.sh | sh` installs the latest compatible prebuilt release asset directly from GitHub Releases
- `brew tap btakita/tap && brew install chromium-bridge` installs the tagged release binaries through the `btakita/homebrew-tap` Homebrew tap on macOS/Linux (`x86_64` and `aarch64`)

## Error Handling

- Connection refused → "Browser not responding on {host}:{port}. Is remote debugging enabled?"
- WebSocket timeout → "Command timed out after {timeout}ms"
- Invalid JS → Forward CDP error message

## Security

- CDP port binds to localhost only — no external exposure
- No secrets handled by the CLI itself
- Screenshot output goes to local files only
