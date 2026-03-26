# chromium-bridge

Rust CLI bridging agents to Chromium-based browsers via Chrome DevTools Protocol (CDP).

Works with **Brave**, **Chrome**, and **Chromium** — any browser that speaks CDP.

## Commands

| Command | Description |
|---------|-------------|
| `chromium-bridge check` | Health check — is the debugging port responding? |
| `chromium-bridge list` | List open tabs with URLs |
| `chromium-bridge screenshot <url>` | Capture a page screenshot |
| `chromium-bridge navigate <url>` | Open a URL in a new tab |
| `chromium-bridge evaluate <js>` | Run JavaScript in the active tab |
| `chromium-bridge setup` | Configure browser for remote debugging |
| `chromium-bridge markdown <url>` | Convert a web page to markdown |

## Setup

Requires a Chromium-based browser running with `--remote-debugging-port`:

```bash
# One-time setup (Brave)
echo "--remote-debugging-port=9222" >> ~/.config/brave-flags.conf

# Or launch manually
brave --remote-debugging-port=9222
```

## Install

```bash
cargo install chromium-bridge
```

## License

MIT
