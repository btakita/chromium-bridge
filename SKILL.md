# chromium-bridge

CLI for browser automation via Chrome DevTools Protocol (CDP). Direct WebSocket connection — no Puppeteer overhead, no MCP protocol layer.

## Commands

| Command | Description |
|---------|-------------|
| `check` | Health check — is the debugging port responding? |
| `list` | List open browser tabs |
| `navigate <url>` | Navigate a tab to a URL |
| `evaluate '<js>'` | Run JavaScript in a tab |
| `screenshot` | Capture a page screenshot |
| `markdown <url>` | Convert a web page to markdown |
| `click '<selector>'` | Click an element by CSS selector |
| `type '<selector>' '<text>'` | Type text into an element |
| `select-tab '<pattern>'` | Activate a tab by index or pattern |
| `wait '<selector>'` | Wait for a CSS selector to appear |
| `snapshot` | Dump the page accessibility tree |
| `setup` | Configure browser for remote debugging |

## Tab Selector

All commands with `--tab` accept:
- **Index:** `--tab 0` (first tab)
- **Pattern:** `--tab messenger` (substring match on URL or title)

Ambiguous patterns (matching multiple tabs) produce an error listing all matches.

## Click

Finds element by CSS selector, computes center coordinates from box model, dispatches real mouse events (mouseMoved, mousePressed, mouseReleased).

```bash
chromium-bridge click 'button.submit'
chromium-bridge click '[data-testid=send-btn]' --tab gmail
```

## Type

Focuses element by CSS selector, types text via CDP `Input.insertText`. **Paragraph handling:** double-newlines (`\n\n`) in the text are converted to Shift+Enter keypresses — this creates visible line breaks in contenteditable fields (Messenger, Gmail compose, Slack, etc.) without triggering "send".

```bash
# Simple input
chromium-bridge type 'input[name=search]' 'hello world'

# Multi-paragraph in contenteditable
chromium-bridge type '[role=textbox]' 'First paragraph.

Second paragraph.

Third paragraph.' --tab messenger
```

## Wait

Polls for a CSS selector to appear in the DOM. Default timeout: 10 seconds.

```bash
chromium-bridge wait 'div.loaded'
chromium-bridge wait '.results' --wait-timeout 30000 --tab 0
```

## Select Tab

Activates a browser tab by bringing it to the foreground.

```bash
chromium-bridge select-tab messenger
chromium-bridge select-tab 0
chromium-bridge select-tab linkedin
```

## Snapshot

Dumps the page accessibility tree. Human-readable output shows `[role] name` for each non-ignored, non-generic node. JSON mode returns the full AXNode array.

```bash
chromium-bridge snapshot --tab messenger
chromium-bridge snapshot --depth 5 --json
```

## Common Patterns

### Send a message in a chat app

```bash
chromium-bridge select-tab messenger
chromium-bridge click '[role=textbox]' --tab messenger
chromium-bridge type '[role=textbox]' 'Hello!

How are you?' --tab messenger
chromium-bridge screenshot -o /tmp/preview.png --tab messenger
```

### Fill a form

```bash
chromium-bridge click 'input[name=email]' --tab mysite
chromium-bridge type 'input[name=email]' 'user@example.com' --tab mysite
chromium-bridge click 'button[type=submit]' --tab mysite
```

### Navigate, wait, then act

```bash
chromium-bridge navigate 'https://example.com'
chromium-bridge wait '.main-content'
chromium-bridge snapshot
```

## Configuration

| Item | Default |
|------|---------|
| CDP host | `127.0.0.1` (env: `CHROMIUM_BRIDGE_HOST`) |
| CDP port | `9222` (env: `CHROMIUM_BRIDGE_PORT`) |
| Timeout | `5000ms` (`--timeout`) |
| JSON output | `--json` flag on any command |

## Prerequisites

A Chromium-based browser running with `--remote-debugging-port=9222`. Run `chromium-bridge setup` to check and configure.
