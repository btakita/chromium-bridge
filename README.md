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
| `chromium-bridge snapshot` | Dump the page accessibility tree with stable refs |
| `chromium-bridge network list` | Reload or navigate and list captured network requests |
| `chromium-bridge network inspect <matcher>` | Reload or navigate and inspect one matched request |
| `chromium-bridge ingest <url>` | Write extracted page markdown into a corky conversations corpus |
| `chromium-bridge state save <name>` | Save cookies plus local/session storage |
| `chromium-bridge state load <name>` | Restore cookies plus local/session storage |
| `chromium-bridge state list` | List saved state snapshots |
| `chromium-bridge setup` | Configure browser for remote debugging |
| `chromium-bridge skill install` | Install SKILL.md to the project's Claude skills |

All commands accept `--tab <index|pattern>` to target a specific tab, `--json` for machine-readable output, and `--timeout <ms>` to override the default 5s timeout.

## Accessibility Snapshot

Use `snapshot` when you need an agent-friendly view of the current page's accessibility tree.

```bash
# Human-readable tree with stable refs on each visible node
chromium-bridge snapshot --tab messenger

# Normalized JSON with ref / parentRef / childRefs
chromium-bridge snapshot --depth 5 --json

# Raw AXNode protocol payload for low-level debugging
chromium-bridge snapshot --depth 5 --json --raw
```

Default text output skips ignored/generic wrappers but preserves tree shape and prints `ref=dom:<id>` (or `ref=ax:<nodeId>` when no DOM backend id is available). JSON mode returns a normalized object with stable refs so follow-up tooling can point back at the same element without depending on array position.

## Network Capture

Use the `network` command family when you need request-level visibility during a page load.

```bash
# Reload the current tab and print captured requests
chromium-bridge network list --tab linkedin

# Navigate to a URL, then inspect the last matching request in detail
chromium-bridge network inspect graphql --url https://www.linkedin.com/feed/ --tab linkedin
```

`network list` and `network inspect` capture requests within a single CDP session so response bodies remain available for inspection. If `--url` is omitted, the command reloads the target tab. Use `--capture-timeout <ms>` and `--idle-ms <ms>` when heavy pages need a longer capture window.

## Corky Ingest

Use `ingest` when you want page content to land directly in corky's `mail/conversations/` corpus instead of manually redirecting stdout to a file.

```bash
# Resolve the corky data dir with corky's normal rules and write one conversation file
chromium-bridge ingest https://example.com/docs/intro

# Add routing metadata and push the new file to Ragie immediately
chromium-bridge ingest https://example.com/docs/intro \
  --labels web,research \
  --accounts chromium-bridge,personal \
  --route \
  --ragie-push

# Keep the shared root corpus, and also copy into a project mailbox corpus
chromium-bridge ingest https://example.com/docs/intro \
  --mailbox project-alpha \
  --labels web,project-alpha
```

By default the command writes a corky-style markdown file into the resolved `conversations/` directory, using the page title as the subject and a stable URL-derived filename slug. Pass `--corky-data` to target a different corky mailbox root, `--slug` to override the filename, or `--title` to override the generated subject. `--mailbox <name>` also copies the same file into `mailboxes/<name>/conversations/` so one ingest can feed both the shared corpus and a project-specific corpus. `--route` runs `corky sync routes` after the root write so label-based mailbox fanout still happens immediately. `--ragie-push` runs `corky ragie push` for the same mailbox root.

For multi-project local search, point separate corky sift backends at those mailbox corpora:

```toml
[search]
default_backends = ["shared", "project-alpha"]

[search.backends.shared]
type = "sift"
index_path = "conversations"

[search.backends.project-alpha]
type = "sift"
index_path = "mailboxes/project-alpha/conversations"
```

That keeps the shared root corpus searchable while giving each project an isolated mailbox corpus for tighter retrieval.

## Persistent State

Use named state snapshots for lightweight persistent browser profiles across sessions:

```bash
# Save cookies + storage from the current LinkedIn tab
chromium-bridge state save linkedin-auth --tab linkedin

# Restore that state later into a fresh tab
chromium-bridge state load linkedin-auth --tab linkedin

# Inspect saved snapshots
chromium-bridge state list
```

Named snapshots are stored under `~/.config/chromium-bridge/states/` by default. Override that with `CHROMIUM_BRIDGE_STATE_DIR=/path/to/states`. Use `--path /tmp/custom.json` on `state save/load` when you want an explicit file location.

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
# From Homebrew
brew tap btakita/tap
brew install chromium-bridge

# From crates.io
cargo install chromium-bridge

# Or via install script (prebuilt binaries)
curl -sSf https://raw.githubusercontent.com/btakita/chromium-bridge/main/install.sh | sh
```

The Homebrew tap ships prebuilt binaries from tagged GitHub releases for macOS and Linux on `x86_64` and `aarch64`.

## License

MIT
