# chromium-bridge

Rust CLI bridging agents to Chromium-based browsers via Chrome DevTools Protocol (CDP).

Works with **Brave**, **Chrome**, and **Chromium** — any browser that speaks CDP.

## Commands

| Command | Description |
|---------|-------------|
| `chromium-bridge check` | Health check — is the debugging port responding? |
| `chromium-bridge list` | List open tabs with URLs |
| `chromium-bridge navigate <url>` | Navigate a tab to a URL |
| `chromium-bridge evaluate <js>` | Run JavaScript expression in a tab |
| `chromium-bridge screenshot [url]` | Capture a page screenshot |
| `chromium-bridge markdown <url>` | Convert a web page to markdown |
| `chromium-bridge click <selector>` | Click an element by CSS selector |
| `chromium-bridge type <selector> <text>` | Type text into an element |
| `chromium-bridge select-tab <pattern>` | Activate a tab by index or pattern |
| `chromium-bridge wait <selector>` | Wait for a CSS selector to appear |
| `chromium-bridge snapshot` | Dump the page accessibility tree |
| `chromium-bridge setup` | Configure browser for remote debugging |
| `chromium-bridge skill install` | Install SKILL.md to the project's Claude skills |

All commands accept `--tab <index|pattern>` to target a specific tab, `--json` for machine-readable output, and `--timeout <ms>` to override the default 5s timeout.

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
# From crates.io
cargo install chromium-bridge

# Or via install script (prebuilt binaries)
curl -sSf https://raw.githubusercontent.com/btakita/chromium-bridge/main/install.sh | sh
```

## License

MIT
