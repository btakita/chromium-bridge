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
| `network list` | Reload or navigate and list captured network requests |
| `network inspect '<matcher>'` | Reload or navigate and inspect one matched request |
| `ingest <url>` | Write extracted page markdown into a corky conversations corpus |
| `state save <name>` | Save cookies plus local/session storage |
| `state load <name>` | Restore cookies plus local/session storage |
| `state list` | List saved state snapshots |
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

Dumps the page accessibility tree. Human-readable output preserves tree shape and includes a stable `ref=` handle for each visible node. JSON mode returns a normalized snapshot object with `ref`, `parentRef`, and `childRefs`. Add `--raw` when you need the underlying AXNode protocol payload.

```bash
chromium-bridge snapshot --tab messenger
chromium-bridge snapshot --depth 5 --json
chromium-bridge snapshot --depth 5 --json --raw
```

## Network

Capture network traffic for a page load when you need request ids, headers, status codes, or response bodies.

```bash
# Reload the current tab and print the captured requests
chromium-bridge network list --tab linkedin

# Navigate first, then inspect the last matching request
chromium-bridge network inspect graphql --url https://www.linkedin.com/feed/ --tab linkedin
```

If `--url` is omitted, the command reloads the target tab. Use `--capture-timeout` and `--idle-ms` on heavy pages that keep loading after first paint.

## Ingest

Use `ingest` when the page should be searchable through corky/sift/ragie without manually redirecting `markdown` output into `mail/conversations/`.

```bash
# Write into the default corky mailbox root
chromium-bridge ingest https://example.com/docs/intro

# Fan out routed mailboxes and push the new file to Ragie
chromium-bridge ingest https://example.com/docs/intro --labels web,research --route --ragie-push

# Also mirror the same page into a project-specific mailbox corpus
chromium-bridge ingest https://example.com/docs/intro --mailbox project-alpha --labels web,project-alpha
```

The command writes a corky-style markdown conversation file into the resolved corky `conversations/` directory. Pass `--corky-data` to target a different mailbox root, `--slug` to control the filename, or `--title` to override the generated subject. `--mailbox <name>` also copies the same file into `mailboxes/<name>/conversations/`, which is useful when corky's sift backends are split into per-project corpora for tighter search results.

## State

Save and restore lightweight persistent browser profiles for authenticated workflows. Each snapshot stores cookies applicable to the current page plus `localStorage` and `sessionStorage` for the current origin.

```bash
# Save the current app session
chromium-bridge state save linkedin-auth --tab linkedin

# Restore it later into a fresh tab
chromium-bridge state load linkedin-auth --tab linkedin

# Inspect what is available
chromium-bridge state list
```

Named snapshots live under `~/.config/chromium-bridge/states/` by default. Override that directory with `CHROMIUM_BRIDGE_STATE_DIR=/path/to/states`, or use `--path /tmp/custom.json` on `state save/load` for an explicit file.

## Chatbox Formatting

When composing rich messages in browser chat apps, use `evaluate` with `innerHTML` to set formatted content. The `type` command only handles plain text with paragraph breaks.

### Contenteditable fields (LinkedIn, Gmail, Slack, Messenger)

Set content via `innerHTML` with `<p>` tags for paragraphs and `<strong>` for bold. Always dispatch an `input` event so the app registers the change.

```bash
chromium-bridge evaluate --tab messaging '
var input = document.querySelector("div[contenteditable=true]");
var lines = [
  "<p><strong>Section header:</strong> paragraph text here.</p>",
  "<p><br></p>",
  "<p>Next paragraph.</p>"
];
input.innerHTML = lines.join("");
input.dispatchEvent(new Event("input", {bubbles: true}));
"done";
'
```

**Platform notes:**
- **LinkedIn messaging:** `<strong>` renders bold. `<p>` per line, `<p><br></p>` for blank lines. Selector: `div.msg-form__contenteditable[contenteditable=true]`
- **Gmail compose:** `<b>` or `<strong>` for bold, `<i>` for italic. Selector: `div[aria-label="Message Body"][contenteditable=true]`
- **Facebook Messenger:** contenteditable `div[role=textbox]`. Bold not supported in plain messages.
- **General pattern:** Find the contenteditable element, set `innerHTML`, dispatch `input` event with `{bubbles: true}`

### Clicking conversation items by name

LinkedIn and similar chat lists use click handlers on `<li>` items, not `<a>` links. Use `evaluate` to find by heading text:

```bash
chromium-bridge evaluate --tab messaging '
var h3 = Array.from(document.querySelectorAll("li h3"))
  .find(h => h.textContent.trim() === "Contact Name");
h3.scrollIntoView({block: "center"});
h3.click();
"clicked";
'
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
| State dir | `~/.config/chromium-bridge/states` (env: `CHROMIUM_BRIDGE_STATE_DIR`) |
| Timeout | `5000ms` (`--timeout`) |
| JSON output | `--json` flag on any command |

## Prerequisites

A Chromium-based browser running with `--remote-debugging-port=9222`. Run `chromium-bridge setup` to check and configure.
