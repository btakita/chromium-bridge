//! # Module: main (chromium-bridge CLI)
//!
//! ## Spec
//! - Entry point for the `chromium-bridge` binary; parses CLI with `clap` derive.
//! - `Cli` struct holds global options (`--host`, `--port`, `--timeout`, `--json`)
//!   and a `Command` subcommand enum.
//! - Commands: `Check`, `List`, `Navigate`, `Evaluate`, `Screenshot`, `Markdown`, `Setup`,
//!   `Click`, `Type`, `SelectTab`, `Wait`, `Snapshot`, `Network`, `Ingest`, `State`, `Skill`.
//! - CDP communication: HTTP (`/json/*`) for tab listing/version, WebSocket via `cdpkit`
//!   for page-level commands (navigate, evaluate, screenshot, input).
//! - `connect_to_tab` creates a cdpkit `CDP` client and attaches to a specific tab by index.
//! - Screenshot and markdown commands use `LoadEventFired` event streaming for page load
//!   detection instead of fixed delays.
//! - `cmd_markdown` injects a DOM walker JS that converts page content to clean markdown.
//! - `cmd_ingest` writes extracted page markdown into a corky conversations corpus and can
//!   optionally trigger `corky sync routes` / `corky ragie push`.
//! - `state save/load/list` persists cookies plus local/session storage for repeatable
//!   authenticated workflows and lightweight profile-like reuse.
//! - `cmd_setup` detects installed Chromium browsers and checks debugging flag status.
//! - `cmd_skill_install` writes the bundled SKILL.md (embedded via `include_str!`) to
//!   `.claude/skills/chromium-bridge/SKILL.md` in the project root.
//!
//! ## Agentic Contracts
//! - All commands return `anyhow::Result<()>`; errors propagate to stderr.
//! - `--json` flag produces machine-readable output on all commands; human-readable by default.
//! - `--tab` accepts an index (e.g., `0`) or URL/title substring pattern (e.g., `linkedin`).
//!   Ambiguous patterns (multiple matches) produce an error listing matches.
//! - CDP connection timeout is configurable via `--timeout` (default 5000ms).
//! - Env vars `CHROMIUM_BRIDGE_HOST` and `CHROMIUM_BRIDGE_PORT` override defaults.
//! - Env var `CHROMIUM_BRIDGE_STATE_DIR` overrides the default state snapshot directory.
//!
//! ## Evals
//! - check_responds: `chromium-bridge check` with live browser → prints OK + version
//! - check_no_browser: `chromium-bridge check` with no browser → error message + exit 1
//! - list_tabs: `chromium-bridge list` → enumerates open page tabs
//! - list_json: `chromium-bridge list --json` → valid JSON array output
//! - navigate_url: `chromium-bridge navigate <url>` → tab navigated, confirmation printed
//! - evaluate_returns_value: `chromium-bridge evaluate "1+1"` → prints "2"
//! - screenshot_file: `chromium-bridge screenshot --output /tmp/test.png` → PNG written
//! - markdown_extraction: `chromium-bridge markdown <url>` → markdown string on stdout
//! - setup_detects_browsers: `chromium-bridge setup` → lists installed Chromium browsers
//! - tab_by_url_pattern: `--tab linkedin` → selects tab whose URL contains "linkedin"
//! - tab_ambiguous_error: `--tab` matches 3 tabs → error listing all matches
//! - click_by_selector: `chromium-bridge click "button.submit"` → clicks element
//! - type_into_element: `chromium-bridge type "input.search" "hello"` → types text
//! - type_with_newlines: text with `\n\n` → Shift+Enter inserted between paragraphs
//! - select_tab_by_pattern: `chromium-bridge select-tab linkedin` → activates matching tab
//! - wait_for_selector: `chromium-bridge wait "div.loaded"` → waits until element exists
//! - snapshot_ax_tree: `chromium-bridge snapshot` → prints accessibility tree with stable refs
//! - snapshot_structured_json: `chromium-bridge snapshot --json` → emits normalized nodes with `ref`, `parentRef`, and `childRefs`
//! - snapshot_raw_ax: `chromium-bridge snapshot --json --raw` → emits raw AXNode protocol objects
//! - network_list_reload: `chromium-bridge network list --tab linkedin` → reloads tab and lists requests
//! - network_inspect_match: `chromium-bridge network inspect graphql --tab linkedin` → reloads tab and prints matched request details
//! - ingest_writes_corky_doc: `chromium-bridge ingest https://example.com` → writes a corky-style markdown doc into `mail/conversations/`
//! - state_save_named: `chromium-bridge state save linkedin-auth --tab linkedin` → writes JSON snapshot
//! - state_load_named: `chromium-bridge state load linkedin-auth --tab linkedin` → restores cookies + storage
//! - state_list: `chromium-bridge state list` → enumerates named snapshots
//! - skill_install: `chromium-bridge skill install` → writes SKILL.md to project .claude/skills/
//! - skill_check: `chromium-bridge skill check` → reports if installed version matches binary

use anyhow::{Context, Result, bail};
use base64::Engine;
use cdpkit::CDP;
use chrono::Utc;
use clap::{Parser, Subcommand};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "chromium-bridge",
    about = "Bridge agents to Chromium browsers via CDP"
)]
struct Cli {
    /// CDP host
    #[arg(long, default_value = "127.0.0.1", env = "CHROMIUM_BRIDGE_HOST")]
    host: String,

    /// CDP port
    #[arg(long, default_value = "9222", env = "CHROMIUM_BRIDGE_PORT")]
    port: u16,

    /// Connection timeout in milliseconds
    #[arg(long, default_value = "5000")]
    timeout: u64,

    /// Output JSON instead of human-readable text
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Health check — is the debugging port responding?
    Check,
    /// List open tabs
    List,
    /// Navigate to a URL
    Navigate {
        /// URL to open
        url: String,
        /// Target tab: index number or URL substring pattern (default: 0)
        #[arg(long, default_value = "0")]
        tab: String,
    },
    /// Run JavaScript in the active tab
    Evaluate {
        /// JavaScript expression to evaluate
        expression: String,
        /// Target tab: index number or URL substring pattern (default: 0)
        #[arg(long, default_value = "0")]
        tab: String,
    },
    /// Capture a page screenshot
    Screenshot {
        /// URL to navigate to before capturing (optional)
        url: Option<String>,
        /// Output file path (default: stdout as base64)
        #[arg(short, long)]
        output: Option<String>,
        /// Target tab: index number or URL substring pattern (default: 0)
        #[arg(long, default_value = "0")]
        tab: String,
    },
    /// Convert a web page to markdown
    Markdown {
        /// URL to convert
        url: String,
        /// Target tab: index number or URL substring pattern (default: 0)
        #[arg(long, default_value = "0")]
        tab: String,
    },
    /// Click an element by CSS selector
    Click {
        /// CSS selector for the element to click
        selector: String,
        /// Target tab: index number or URL substring pattern (default: 0)
        #[arg(long, default_value = "0")]
        tab: String,
    },
    /// Type text into a focused or selected element
    Type {
        /// CSS selector for the element to type into
        selector: String,
        /// Text to type (use \n for newlines in contenteditable; sends Shift+Enter)
        text: String,
        /// Target tab: index number or URL substring pattern (default: 0)
        #[arg(long, default_value = "0")]
        tab: String,
    },
    /// Activate a browser tab by index or URL/title pattern
    SelectTab {
        /// Tab selector: index number or URL/title substring pattern
        selector: String,
    },
    /// Wait for a CSS selector to appear in the DOM
    Wait {
        /// CSS selector to wait for
        selector: String,
        /// Timeout in milliseconds (default: 10000)
        #[arg(long, default_value = "10000")]
        wait_timeout: u64,
        /// Target tab: index number or URL substring pattern (default: 0)
        #[arg(long, default_value = "0")]
        tab: String,
    },
    /// Dump the page accessibility tree
    Snapshot {
        /// Maximum depth of the tree (default: unlimited)
        #[arg(long)]
        depth: Option<i64>,
        /// Emit the raw AXNode protocol payload instead of normalized snapshot JSON
        #[arg(long)]
        raw: bool,
        /// Target tab: index number or URL substring pattern (default: 0)
        #[arg(long, default_value = "0")]
        tab: String,
    },
    /// Capture and inspect network requests for a page load
    Network {
        #[command(subcommand)]
        action: NetworkAction,
    },
    /// Extract a page into a corky conversations corpus
    Ingest {
        /// URL to ingest
        url: String,
        /// Optional subject/title override
        #[arg(long)]
        title: Option<String>,
        /// Optional filename slug override (without .md)
        #[arg(long)]
        slug: Option<String>,
        /// Comma-separated labels to write into the corky document
        #[arg(long, default_value = "web")]
        labels: String,
        /// Comma-separated account names to write into the corky document
        #[arg(long, default_value = "chromium-bridge")]
        accounts: String,
        /// Optional explicit corky data dir (defaults to corky's normal resolution order)
        #[arg(long)]
        corky_data: Option<String>,
        /// Run `corky sync routes` after writing the document
        #[arg(long)]
        route: bool,
        /// Run `corky ragie push` after writing the document
        #[arg(long)]
        ragie_push: bool,
        /// Use `corky ragie push --full` instead of the default fast mode
        #[arg(long)]
        ragie_full: bool,
        /// Target tab: index number or URL substring pattern (default: 0)
        #[arg(long, default_value = "0")]
        tab: String,
    },
    /// Save or restore persistent browser state snapshots
    State {
        #[command(subcommand)]
        action: StateAction,
    },
    /// Manage the Claude Code skill definition
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
    /// Configure browser for remote debugging
    Setup,
}

#[derive(Subcommand)]
enum SkillAction {
    /// Install SKILL.md to .claude/skills/chromium-bridge/
    Install,
    /// Check if installed skill matches this binary version
    Check,
}

#[derive(Subcommand)]
enum NetworkAction {
    /// Reload or navigate a tab, then list captured network requests
    List {
        /// Optional URL to navigate to before capture; otherwise reloads the target tab
        #[arg(long)]
        url: Option<String>,
        /// Capture timeout in milliseconds (default: 15000)
        #[arg(long, default_value = "15000")]
        capture_timeout: u64,
        /// Stop after this many idle milliseconds following page load (default: 750)
        #[arg(long, default_value = "750")]
        idle_ms: u64,
        /// Maximum number of requests to print
        #[arg(long)]
        limit: Option<usize>,
        /// Target tab: index number or URL substring pattern (default: 0)
        #[arg(long, default_value = "0")]
        tab: String,
    },
    /// Reload or navigate a tab, then inspect one matched request in detail
    Inspect {
        /// Exact request id or URL substring to inspect
        matcher: String,
        /// Optional URL to navigate to before capture; otherwise reloads the target tab
        #[arg(long)]
        url: Option<String>,
        /// Capture timeout in milliseconds (default: 15000)
        #[arg(long, default_value = "15000")]
        capture_timeout: u64,
        /// Stop after this many idle milliseconds following page load (default: 750)
        #[arg(long, default_value = "750")]
        idle_ms: u64,
        /// Maximum number of characters to print from the response body (0 = unlimited)
        #[arg(long, default_value = "4000")]
        body_limit: usize,
        /// Target tab: index number or URL substring pattern (default: 0)
        #[arg(long, default_value = "0")]
        tab: String,
    },
}

#[derive(Subcommand)]
enum StateAction {
    /// Save cookies plus local/session storage into a named snapshot
    Save {
        /// Snapshot name; saved under the state dir unless --path is supplied
        name: String,
        /// Optional explicit output path
        #[arg(long)]
        path: Option<String>,
        /// Target tab: index number or URL substring pattern (default: 0)
        #[arg(long, default_value = "0")]
        tab: String,
    },
    /// Restore cookies plus local/session storage from a snapshot
    Load {
        /// Snapshot name; loaded from the state dir unless --path is supplied
        name: String,
        /// Optional explicit input path
        #[arg(long)]
        path: Option<String>,
        /// Target tab: index number or URL substring pattern (default: 0)
        #[arg(long, default_value = "0")]
        tab: String,
    },
    /// List saved snapshots in the local state dir
    List,
}

#[derive(Deserialize, Serialize)]
struct BrowserVersion {
    #[serde(rename = "Browser")]
    browser: String,
    #[serde(rename = "Protocol-Version")]
    protocol_version: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    #[serde(default)]
    web_socket_debugger_url: String,
}

#[derive(Deserialize, Serialize)]
struct Tab {
    id: String,
    title: String,
    url: String,
    #[serde(rename = "type")]
    tab_type: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    #[serde(default)]
    web_socket_debugger_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedBrowserState {
    version: u32,
    saved_at_unix_ms: u64,
    active_url: String,
    active_title: String,
    cookies: Vec<cdpkit::network::types::CookieParam>,
    origins: Vec<OriginStorageState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OriginStorageState {
    origin: String,
    url: String,
    title: String,
    local_storage: BTreeMap<String, String>,
    session_storage: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageStorageSnapshot {
    href: String,
    origin: String,
    title: String,
    local_storage: BTreeMap<String, String>,
    session_storage: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StateListEntry {
    name: String,
    path: String,
}

#[derive(Debug, Clone)]
struct CapturedNetworkRequest {
    request_id: String,
    document_url: String,
    url: String,
    method: String,
    resource_type: Option<String>,
    initiator_type: String,
    initiator_url: Option<String>,
    request_headers: serde_json::Value,
    has_post_data: bool,
    post_data: Option<String>,
    response_status: Option<i64>,
    response_status_text: Option<String>,
    response_headers: Option<serde_json::Value>,
    mime_type: Option<String>,
    protocol: Option<String>,
    remote_ip_address: Option<String>,
    remote_port: Option<i64>,
    from_disk_cache: bool,
    from_service_worker: bool,
    encoded_data_length: Option<f64>,
    failed: bool,
    canceled: bool,
    error_text: Option<String>,
    finished: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkRequestSummary {
    request_id: String,
    document_url: String,
    url: String,
    method: String,
    resource_type: Option<String>,
    initiator_type: String,
    initiator_url: Option<String>,
    status: Option<i64>,
    status_text: Option<String>,
    mime_type: Option<String>,
    protocol: Option<String>,
    remote_ip_address: Option<String>,
    remote_port: Option<i64>,
    from_disk_cache: bool,
    from_service_worker: bool,
    encoded_data_length: Option<f64>,
    failed: bool,
    canceled: bool,
    error_text: Option<String>,
    finished: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkListOutput {
    trigger: String,
    request_count: usize,
    requests: Vec<NetworkRequestSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkBodyOutput {
    content: String,
    encoding: String,
    was_base64_encoded: bool,
    truncated: bool,
    original_length: usize,
    decoded_byte_length: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkInspectOutput {
    trigger: String,
    matcher: String,
    matched_by: String,
    matched_count: usize,
    request: NetworkRequestSummary,
    request_headers: serde_json::Value,
    response_headers: Option<serde_json::Value>,
    request_post_data: Option<String>,
    request_post_data_error: Option<String>,
    response_body: Option<NetworkBodyOutput>,
    response_body_error: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum NetworkMatchKind {
    RequestId,
    UrlPattern,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotOutput {
    root_refs: Vec<String>,
    node_count: usize,
    nodes: Vec<SnapshotNodeOutput>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotNodeOutput {
    #[serde(rename = "ref")]
    ref_id: String,
    ax_node_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    backend_dom_node_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_ref: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    child_refs: Vec<String>,
    ignored: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chrome_role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<SnapshotValueOutput>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    properties: BTreeMap<String, SnapshotValueOutput>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct SnapshotValueOutput {
    #[serde(rename = "type")]
    type_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    related_refs: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    related_text: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtractedPage {
    url: String,
    title: String,
    markdown: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct IngestOutput {
    path: String,
    corky_data_dir: String,
    slug: String,
    subject: String,
    thread_id: String,
    source_url: String,
    routed: bool,
    ragie_pushed: bool,
}

struct IngestDocument<'a> {
    subject: &'a str,
    labels: &'a [String],
    accounts: &'a [String],
    thread_id: &'a str,
    source_url: &'a str,
    source_title: &'a str,
    sender: &'a str,
    timestamp: &'a str,
    markdown: &'a str,
}

struct IngestRequest<'a> {
    url: &'a str,
    title_override: Option<&'a str>,
    slug_override: Option<&'a str>,
    labels_raw: &'a str,
    accounts_raw: &'a str,
    corky_data_override: Option<&'a str>,
    route: bool,
    ragie_push: bool,
    ragie_full: bool,
    tab_selector: &'a str,
}

fn base_url(cli: &Cli) -> String {
    format!("http://{}:{}", cli.host, cli.port)
}

fn client(cli: &Cli) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(cli.timeout))
        .build()
        .expect("failed to build HTTP client")
}

async fn get_tabs(cli: &Cli) -> Result<Vec<Tab>> {
    let resp = client(cli)
        .get(format!("{}/json/list", base_url(cli)))
        .send()
        .await
        .context(format!(
            "Browser not responding on {}:{}. Is remote debugging enabled?",
            cli.host, cli.port
        ))?;
    let tabs: Vec<Tab> = resp.json().await?;
    Ok(tabs)
}

fn expand_path_with_home(path: &str, home: Option<&str>) -> Result<PathBuf> {
    if let Some(stripped) = path.strip_prefix("~/") {
        let home = home.context("HOME is not set")?;
        return Ok(PathBuf::from(home).join(stripped));
    }
    Ok(PathBuf::from(path))
}

fn expand_path(path: &str) -> Result<PathBuf> {
    let home = std::env::var("HOME").ok();
    expand_path_with_home(path, home.as_deref())
}

fn resolve_corky_data_dir_from(
    explicit_dir: Option<&str>,
    cwd: &Path,
    env_dir: Option<&str>,
    home: Option<&str>,
) -> Result<PathBuf> {
    if let Some(path) = explicit_dir {
        return expand_path_with_home(path, home);
    }
    if cwd.join(".corky.toml").exists() || cwd.join("corky.toml").exists() {
        return Ok(cwd.to_path_buf());
    }
    let local_mail = cwd.join("mail");
    if local_mail.is_dir() {
        return Ok(local_mail);
    }
    if let Some(path) = env_dir
        && !path.is_empty()
    {
        return expand_path_with_home(path, home);
    }
    let home = home.context("HOME is not set")?;
    Ok(PathBuf::from(home).join("Documents").join("mail"))
}

fn resolve_corky_data_dir(explicit_dir: Option<&str>) -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("Failed to read current directory")?;
    let env_dir = std::env::var("CORKY_DATA").ok();
    let home = std::env::var("HOME").ok();
    resolve_corky_data_dir_from(explicit_dir, &cwd, env_dir.as_deref(), home.as_deref())
}

fn state_root_from_env(
    override_dir: Option<&str>,
    xdg_config_home: Option<&str>,
    home: Option<&str>,
) -> Result<PathBuf> {
    if let Some(dir) = override_dir {
        return expand_path_with_home(dir, home);
    }

    if let Some(config_home) = xdg_config_home {
        return Ok(PathBuf::from(config_home)
            .join("chromium-bridge")
            .join("states"));
    }

    let home = home.context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("chromium-bridge")
        .join("states"))
}

fn state_root() -> Result<PathBuf> {
    let override_dir = std::env::var("CHROMIUM_BRIDGE_STATE_DIR").ok();
    let xdg_config_home = std::env::var("XDG_CONFIG_HOME").ok();
    let home = std::env::var("HOME").ok();
    state_root_from_env(
        override_dir.as_deref(),
        xdg_config_home.as_deref(),
        home.as_deref(),
    )
}

fn validate_state_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        bail!("Snapshot name cannot be empty");
    }
    if name.contains('/') || name.contains('\\') {
        bail!("Snapshot names cannot contain path separators; use --path for explicit file paths");
    }
    Ok(())
}

fn normalize_csv_values(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn slugify_token(raw: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for ch in raw.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            previous_dash = false;
        } else if !previous_dash && !slug.is_empty() {
            slug.push('-');
            previous_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn default_ingest_slug(url: &str, title: &str) -> String {
    if let Ok(parsed) = reqwest::Url::parse(url) {
        let mut parts = Vec::new();
        if let Some(host) = parsed.host_str() {
            let host_slug = slugify_token(host);
            if !host_slug.is_empty() {
                parts.push(host_slug);
            }
        }
        for segment in parsed
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .take(6)
        {
            let segment_slug = slugify_token(segment);
            if !segment_slug.is_empty() {
                parts.push(segment_slug);
            }
        }
        if !parts.is_empty() {
            return parts.join("-");
        }
    }

    let title_slug = slugify_token(title);
    if !title_slug.is_empty() {
        title_slug
    } else {
        "web-page".to_string()
    }
}

fn resolve_ingest_slug(explicit_slug: Option<&str>, url: &str, title: &str) -> Result<String> {
    let slug = explicit_slug
        .map(slugify_token)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_ingest_slug(url, title));
    if slug.is_empty() {
        bail!("Unable to derive a filename slug; pass --slug explicitly");
    }
    Ok(slug)
}

fn render_ingest_document(document: &IngestDocument<'_>) -> String {
    let labels_line = document.labels.join(", ");
    let accounts_line = document.accounts.join(", ");
    format!(
        "# {subject}\n\n**Labels**: {labels_line}\n**Accounts**: {accounts_line}\n**Thread ID**: {thread_id}\n**Last updated**: {timestamp}\n**Source URL**: {source_url}\n**Source Title**: {source_title}\n\n---\n\n## {sender} — {timestamp}\n\n{body}\n",
        subject = document.subject,
        thread_id = document.thread_id,
        timestamp = document.timestamp,
        source_url = document.source_url,
        source_title = document.source_title,
        sender = document.sender,
        body = document.markdown.trim()
    )
}

fn infer_ingest_sender(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(ToString::to_string))
        .unwrap_or_else(|| "web".to_string())
}

fn run_corky_command(data_dir: &Path, args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("corky")
        .args(args)
        .env("CORKY_DATA", data_dir)
        .output()
        .with_context(|| format!("Failed to run 'corky {}'", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "corky {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn resolve_state_path(name: &str, explicit_path: Option<&str>) -> Result<PathBuf> {
    if let Some(path) = explicit_path {
        return expand_path(path);
    }

    validate_state_name(name)?;
    let file_name = if name.ends_with(".json") {
        name.to_string()
    } else {
        format!("{name}.json")
    };
    Ok(state_root()?.join(file_name))
}

fn cookie_to_param(cookie: cdpkit::network::types::Cookie) -> cdpkit::network::types::CookieParam {
    cdpkit::network::types::CookieParam {
        name: cookie.name,
        value: cookie.value,
        url: None,
        domain: Some(cookie.domain),
        path: Some(cookie.path),
        secure: Some(cookie.secure),
        http_only: Some(cookie.http_only),
        same_site: cookie.same_site,
        expires: if cookie.session || cookie.expires < 0.0 {
            None
        } else {
            Some(cookie.expires)
        },
        priority: Some(cookie.priority),
        source_scheme: Some(cookie.source_scheme),
        source_port: Some(cookie.source_port),
        partition_key: cookie.partition_key,
    }
}

fn network_summary(entry: &CapturedNetworkRequest) -> NetworkRequestSummary {
    NetworkRequestSummary {
        request_id: entry.request_id.clone(),
        document_url: entry.document_url.clone(),
        url: entry.url.clone(),
        method: entry.method.clone(),
        resource_type: entry.resource_type.clone(),
        initiator_type: entry.initiator_type.clone(),
        initiator_url: entry.initiator_url.clone(),
        status: entry.response_status,
        status_text: entry.response_status_text.clone(),
        mime_type: entry.mime_type.clone(),
        protocol: entry.protocol.clone(),
        remote_ip_address: entry.remote_ip_address.clone(),
        remote_port: entry.remote_port,
        from_disk_cache: entry.from_disk_cache,
        from_service_worker: entry.from_service_worker,
        encoded_data_length: entry.encoded_data_length,
        failed: entry.failed,
        canceled: entry.canceled,
        error_text: entry.error_text.clone(),
        finished: entry.finished,
    }
}

fn snapshot_ref(node: &cdpkit::accessibility::types::AXNode) -> String {
    node.backend_dom_node_id
        .map(|id| format!("dom:{id}"))
        .unwrap_or_else(|| format!("ax:{}", node.node_id))
}

fn snapshot_ref_for_related_node(node: &cdpkit::accessibility::types::AXRelatedNode) -> String {
    format!("dom:{}", node.backend_dom_node_id)
}

fn ax_value_text(value: &cdpkit::accessibility::types::AXValue) -> Option<String> {
    match value.value.as_ref()? {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

fn ax_value_summary(value: &cdpkit::accessibility::types::AXValue) -> SnapshotValueOutput {
    let related_refs = value
        .related_nodes
        .as_ref()
        .map(|nodes| nodes.iter().map(snapshot_ref_for_related_node).collect())
        .unwrap_or_default();
    let related_text = value
        .related_nodes
        .as_ref()
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|node| node.text.clone())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    SnapshotValueOutput {
        type_name: value.type_.as_ref().to_string(),
        value: value.value.clone(),
        related_refs,
        related_text,
    }
}

fn ax_value_string(value: Option<&cdpkit::accessibility::types::AXValue>) -> Option<String> {
    value
        .and_then(ax_value_text)
        .filter(|text| !text.is_empty())
}

fn snapshot_role(node: &cdpkit::accessibility::types::AXNode) -> Option<String> {
    ax_value_string(node.role.as_ref())
}

fn snapshot_primary_label(node: &cdpkit::accessibility::types::AXNode) -> Option<String> {
    ax_value_string(node.name.as_ref())
        .or_else(|| ax_value_string(node.value.as_ref()))
        .or_else(|| ax_value_string(node.description.as_ref()))
}

fn should_display_snapshot_node(node: &cdpkit::accessibility::types::AXNode) -> bool {
    if node.ignored {
        return false;
    }

    !matches!(snapshot_role(node).as_deref(), Some("none" | "generic"))
}

fn snapshot_roots<'a>(
    nodes: &'a [cdpkit::accessibility::types::AXNode],
    node_index: &HashMap<&'a str, &'a cdpkit::accessibility::types::AXNode>,
) -> Vec<&'a cdpkit::accessibility::types::AXNode> {
    nodes
        .iter()
        .filter(|node| {
            node.parent_id
                .as_deref()
                .and_then(|parent_id| node_index.get(parent_id).copied())
                .is_none()
        })
        .collect()
}

fn build_snapshot_output(nodes: &[cdpkit::accessibility::types::AXNode]) -> SnapshotOutput {
    let node_index = nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let root_refs = snapshot_roots(nodes, &node_index)
        .into_iter()
        .map(snapshot_ref)
        .collect::<Vec<_>>();

    let normalized_nodes = nodes
        .iter()
        .map(|node| SnapshotNodeOutput {
            ref_id: snapshot_ref(node),
            ax_node_id: node.node_id.clone(),
            backend_dom_node_id: node.backend_dom_node_id,
            parent_ref: node
                .parent_id
                .as_deref()
                .and_then(|parent_id| node_index.get(parent_id).copied())
                .map(snapshot_ref),
            child_refs: node
                .child_ids
                .as_ref()
                .map(|child_ids| {
                    child_ids
                        .iter()
                        .filter_map(|child_id| node_index.get(child_id.as_str()).copied())
                        .map(snapshot_ref)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            ignored: node.ignored,
            role: snapshot_role(node),
            chrome_role: ax_value_string(node.chrome_role.as_ref()),
            name: ax_value_string(node.name.as_ref()),
            description: ax_value_string(node.description.as_ref()),
            value: node.value.as_ref().map(ax_value_summary),
            properties: node
                .properties
                .as_ref()
                .map(|properties| {
                    properties
                        .iter()
                        .map(|property| {
                            (
                                property.name.as_ref().to_string(),
                                ax_value_summary(&property.value),
                            )
                        })
                        .collect::<BTreeMap<_, _>>()
                })
                .unwrap_or_default(),
        })
        .collect::<Vec<_>>();

    SnapshotOutput {
        root_refs,
        node_count: normalized_nodes.len(),
        nodes: normalized_nodes,
    }
}

fn render_snapshot_text(nodes: &[cdpkit::accessibility::types::AXNode]) -> String {
    let node_index = nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let mut lines = Vec::new();
    let mut visited = HashSet::new();

    fn visit(
        node: &cdpkit::accessibility::types::AXNode,
        depth: usize,
        node_index: &HashMap<&str, &cdpkit::accessibility::types::AXNode>,
        visited: &mut HashSet<String>,
        lines: &mut Vec<String>,
    ) {
        if !visited.insert(node.node_id.clone()) {
            return;
        }

        let display = should_display_snapshot_node(node);
        let next_depth = if display { depth + 1 } else { depth };

        if display {
            let role = snapshot_role(node).unwrap_or_else(|| "?".to_string());
            let label = snapshot_primary_label(node)
                .map(|text| {
                    if text.len() > 80 {
                        format!("{}…", &text[..80])
                    } else {
                        text
                    }
                })
                .unwrap_or_default();
            let suffix = if label.is_empty() {
                String::new()
            } else {
                format!(" {label}")
            };

            lines.push(format!(
                "{}[{}]{} (ref={})",
                "  ".repeat(depth),
                role,
                suffix,
                snapshot_ref(node)
            ));
        }

        if let Some(child_ids) = node.child_ids.as_ref() {
            for child_id in child_ids {
                if let Some(child) = node_index.get(child_id.as_str()).copied() {
                    visit(child, next_depth, node_index, visited, lines);
                }
            }
        }
    }

    for root in snapshot_roots(nodes, &node_index) {
        visit(root, 0, &node_index, &mut visited, &mut lines);
    }

    for node in nodes {
        if !visited.contains(node.node_id.as_str()) {
            visit(node, 0, &node_index, &mut visited, &mut lines);
        }
    }

    if lines.is_empty() {
        "No visible accessibility nodes.".to_string()
    } else {
        lines.join("\n")
    }
}

fn upsert_request_event(
    order: &mut Vec<String>,
    requests: &mut HashMap<String, CapturedNetworkRequest>,
    event: cdpkit::network::events::RequestWillBeSent,
) {
    let request_id = event.request_id.clone();
    if !requests.contains_key(&request_id) {
        order.push(request_id.clone());
    }

    let entry = requests
        .entry(request_id.clone())
        .or_insert_with(|| CapturedNetworkRequest {
            request_id: request_id.clone(),
            document_url: String::new(),
            url: String::new(),
            method: String::new(),
            resource_type: None,
            initiator_type: String::new(),
            initiator_url: None,
            request_headers: serde_json::json!({}),
            has_post_data: false,
            post_data: None,
            response_status: None,
            response_status_text: None,
            response_headers: None,
            mime_type: None,
            protocol: None,
            remote_ip_address: None,
            remote_port: None,
            from_disk_cache: false,
            from_service_worker: false,
            encoded_data_length: None,
            failed: false,
            canceled: false,
            error_text: None,
            finished: false,
        });

    entry.document_url = event.document_url;
    entry.url = event.request.url;
    entry.method = event.request.method;
    entry.resource_type = event.type_.map(|value| value.as_ref().to_string());
    entry.initiator_type = event.initiator.type_;
    entry.initiator_url = event.initiator.url;
    entry.request_headers = event.request.headers;
    entry.has_post_data = event.request.has_post_data.unwrap_or(
        event
            .request
            .post_data_entries
            .as_ref()
            .map(|entries| !entries.is_empty())
            .unwrap_or(false),
    );
    entry.post_data = None;

    if event.redirect_response.is_some() {
        entry.response_status = None;
        entry.response_status_text = None;
        entry.response_headers = None;
        entry.mime_type = None;
        entry.protocol = None;
        entry.remote_ip_address = None;
        entry.remote_port = None;
        entry.from_disk_cache = false;
        entry.from_service_worker = false;
        entry.encoded_data_length = None;
        entry.failed = false;
        entry.canceled = false;
        entry.error_text = None;
        entry.finished = false;
    }
}

fn apply_response_event(
    requests: &mut HashMap<String, CapturedNetworkRequest>,
    event: cdpkit::network::events::ResponseReceived,
) {
    if let Some(entry) = requests.get_mut(&event.request_id) {
        entry.resource_type = Some(event.type_.as_ref().to_string());
        entry.response_status = Some(event.response.status);
        entry.response_status_text = Some(event.response.status_text);
        entry.response_headers = Some(event.response.headers);
        entry.mime_type = Some(event.response.mime_type);
        entry.protocol = event.response.protocol;
        entry.remote_ip_address = event.response.remote_ip_address;
        entry.remote_port = event.response.remote_port;
        entry.from_disk_cache = event.response.from_disk_cache.unwrap_or(false);
        entry.from_service_worker = event.response.from_service_worker.unwrap_or(false);
        entry.encoded_data_length = Some(event.response.encoded_data_length);
    }
}

fn apply_loading_finished(
    requests: &mut HashMap<String, CapturedNetworkRequest>,
    event: cdpkit::network::events::LoadingFinished,
) {
    if let Some(entry) = requests.get_mut(&event.request_id) {
        entry.encoded_data_length = Some(event.encoded_data_length);
        entry.finished = true;
    }
}

fn apply_loading_failed(
    requests: &mut HashMap<String, CapturedNetworkRequest>,
    event: cdpkit::network::events::LoadingFailed,
) {
    if let Some(entry) = requests.get_mut(&event.request_id) {
        entry.resource_type = Some(event.type_.as_ref().to_string());
        entry.failed = true;
        entry.canceled = event.canceled.unwrap_or(false);
        entry.error_text = Some(event.error_text);
        entry.finished = true;
    }
}

async fn capture_network_requests(
    cli: &Cli,
    url: Option<&str>,
    capture_timeout_ms: u64,
    idle_ms: u64,
    tab_selector: &str,
) -> Result<(String, Vec<CapturedNetworkRequest>, CDP, String)> {
    let (cdp, session) = connect_to_tab(cli, tab_selector).await?;

    cdpkit::page::methods::Enable::new()
        .send(&cdp, Some(&session))
        .await?;
    cdpkit::network::methods::Enable::new()
        .with_max_total_buffer_size(100_000_000_i64)
        .with_max_resource_buffer_size(10_000_000_i64)
        .with_max_post_data_size(1_000_000_i64)
        .with_enable_durable_messages(true)
        .send(&cdp, Some(&session))
        .await?;

    let mut request_events = cdpkit::network::events::RequestWillBeSent::subscribe(&cdp);
    let mut response_events = cdpkit::network::events::ResponseReceived::subscribe(&cdp);
    let mut finished_events = cdpkit::network::events::LoadingFinished::subscribe(&cdp);
    let mut failed_events = cdpkit::network::events::LoadingFailed::subscribe(&cdp);
    let mut load_events = cdpkit::page::events::LoadEventFired::subscribe(&cdp);

    let trigger = if let Some(url) = url {
        cdpkit::page::methods::Navigate::new(url)
            .send(&cdp, Some(&session))
            .await?;
        format!("navigate:{url}")
    } else {
        cdpkit::page::methods::Reload::new()
            .send(&cdp, Some(&session))
            .await?;
        "reload".to_string()
    };

    let start = std::time::Instant::now();
    let idle_window = std::time::Duration::from_millis(idle_ms);
    let capture_timeout = std::time::Duration::from_millis(capture_timeout_ms);
    let mut saw_load = false;
    let mut last_activity = std::time::Instant::now();
    let mut order = Vec::new();
    let mut requests = HashMap::new();

    loop {
        if saw_load && last_activity.elapsed() >= idle_window {
            break;
        }

        if start.elapsed() >= capture_timeout {
            if !saw_load {
                bail!(
                    "Timed out waiting for page load after {}ms",
                    capture_timeout_ms
                );
            }
            break;
        }

        tokio::select! {
            Some(event) = request_events.next() => {
                upsert_request_event(&mut order, &mut requests, event);
                last_activity = std::time::Instant::now();
            }
            Some(event) = response_events.next() => {
                apply_response_event(&mut requests, event);
                last_activity = std::time::Instant::now();
            }
            Some(event) = finished_events.next() => {
                apply_loading_finished(&mut requests, event);
                last_activity = std::time::Instant::now();
            }
            Some(event) = failed_events.next() => {
                apply_loading_failed(&mut requests, event);
                last_activity = std::time::Instant::now();
            }
            Some(_) = load_events.next() => {
                saw_load = true;
                last_activity = std::time::Instant::now();
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
        }
    }

    let captured = order
        .into_iter()
        .filter_map(|request_id| requests.remove(&request_id))
        .collect::<Vec<_>>();

    Ok((trigger, captured, cdp, session))
}

fn select_network_request<'a>(
    requests: &'a [CapturedNetworkRequest],
    matcher: &str,
) -> Result<(&'a CapturedNetworkRequest, NetworkMatchKind, usize)> {
    if let Some(exact) = requests.iter().find(|entry| entry.request_id == matcher) {
        return Ok((exact, NetworkMatchKind::RequestId, 1));
    }

    let matches = requests
        .iter()
        .filter(|entry| entry.url.contains(matcher))
        .collect::<Vec<_>>();

    let Some(entry) = matches.last().copied() else {
        bail!(
            "No captured request matched '{}'. Use 'chromium-bridge network list' to inspect request ids.",
            matcher
        );
    };

    Ok((entry, NetworkMatchKind::UrlPattern, matches.len()))
}

fn is_text_like_mime(mime_type: Option<&str>) -> bool {
    let Some(mime_type) = mime_type else {
        return false;
    };
    mime_type.starts_with("text/")
        || mime_type.contains("json")
        || mime_type.contains("xml")
        || mime_type.contains("javascript")
        || mime_type.contains("svg")
        || mime_type.contains("x-www-form-urlencoded")
}

fn truncate_text(text: &str, limit: usize) -> (String, bool) {
    if limit == 0 {
        return (text.to_string(), false);
    }

    let mut truncated = String::new();
    for (count, ch) in text.chars().enumerate() {
        if count == limit {
            return (truncated, true);
        }
        truncated.push(ch);
    }
    (truncated, false)
}

fn decode_network_body(
    body: &str,
    base64_encoded: bool,
    mime_type: Option<&str>,
    limit: usize,
) -> NetworkBodyOutput {
    if !base64_encoded {
        let (content, truncated) = truncate_text(body, limit);
        return NetworkBodyOutput {
            content,
            encoding: "plain".to_string(),
            was_base64_encoded: false,
            truncated,
            original_length: body.chars().count(),
            decoded_byte_length: Some(body.len()),
        };
    }

    let decoded = base64::engine::general_purpose::STANDARD.decode(body);
    match decoded {
        Ok(bytes) if is_text_like_mime(mime_type) => match String::from_utf8(bytes.clone()) {
            Ok(text) => {
                let (content, truncated) = truncate_text(&text, limit);
                NetworkBodyOutput {
                    content,
                    encoding: "utf8".to_string(),
                    was_base64_encoded: true,
                    truncated,
                    original_length: text.chars().count(),
                    decoded_byte_length: Some(bytes.len()),
                }
            }
            Err(_) => {
                let (content, truncated) = truncate_text(body, limit);
                NetworkBodyOutput {
                    content,
                    encoding: "base64".to_string(),
                    was_base64_encoded: true,
                    truncated,
                    original_length: body.chars().count(),
                    decoded_byte_length: None,
                }
            }
        },
        Ok(bytes) => {
            let (content, truncated) = truncate_text(body, limit);
            NetworkBodyOutput {
                content,
                encoding: "base64".to_string(),
                was_base64_encoded: true,
                truncated,
                original_length: body.chars().count(),
                decoded_byte_length: Some(bytes.len()),
            }
        }
        Err(_) => {
            let (content, truncated) = truncate_text(body, limit);
            NetworkBodyOutput {
                content,
                encoding: "base64".to_string(),
                was_base64_encoded: true,
                truncated,
                original_length: body.chars().count(),
                decoded_byte_length: None,
            }
        }
    }
}

fn format_size(bytes: Option<f64>) -> String {
    let Some(bytes) = bytes else {
        return "?".to_string();
    };
    if bytes >= 1_000_000.0 {
        format!("{:.1} MB", bytes / 1_000_000.0)
    } else if bytes >= 1_000.0 {
        format!("{:.1} KB", bytes / 1_000.0)
    } else {
        format!("{bytes:.0} B")
    }
}

fn print_json_block(label: &str, value: &serde_json::Value) -> Result<()> {
    println!("{label}:");
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

async fn navigate_and_wait(cdp: &CDP, session: &str, url: &str) -> Result<()> {
    cdpkit::page::methods::Enable::new()
        .send(cdp, Some(session))
        .await?;

    let mut events = cdpkit::page::events::LoadEventFired::subscribe(cdp);
    cdpkit::page::methods::Navigate::new(url)
        .send(cdp, Some(session))
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(10), events.next())
        .await
        .context(format!("Timed out waiting for '{}' to load", url))?
        .context("Page load event stream closed unexpectedly")?;
    Ok(())
}

async fn extract_page_markdown(cdp: &CDP, session: &str) -> Result<ExtractedPage> {
    let js = r#"
    (() => {
        const clone = document.cloneNode(true);
        clone.querySelectorAll('script, style, nav, footer, aside, iframe, noscript').forEach(el => el.remove());

        function nodeToMarkdown(node) {
            if (node.nodeType === Node.TEXT_NODE) {
                return node.textContent.replace(/\s+/g, ' ');
            }
            if (node.nodeType !== Node.ELEMENT_NODE) return '';

            const tag = node.tagName.toLowerCase();
            const children = Array.from(node.childNodes).map(c => nodeToMarkdown(c)).join('');

            switch(tag) {
                case 'h1': return '\n# ' + children.trim() + '\n';
                case 'h2': return '\n## ' + children.trim() + '\n';
                case 'h3': return '\n### ' + children.trim() + '\n';
                case 'h4': return '\n#### ' + children.trim() + '\n';
                case 'h5': return '\n##### ' + children.trim() + '\n';
                case 'h6': return '\n###### ' + children.trim() + '\n';
                case 'p': return '\n' + children.trim() + '\n';
                case 'br': return '\n';
                case 'strong': case 'b': return '**' + children.trim() + '**';
                case 'em': case 'i': return '*' + children.trim() + '*';
                case 'code': return '`' + children.trim() + '`';
                case 'pre': return '\n```\n' + children.trim() + '\n```\n';
                case 'a': {
                    const href = node.getAttribute('href') || '';
                    return '[' + children.trim() + '](' + href + ')';
                }
                case 'img': {
                    const alt = node.getAttribute('alt') || '';
                    const src = node.getAttribute('src') || '';
                    return '![' + alt + '](' + src + ')';
                }
                case 'li': return '- ' + children.trim() + '\n';
                case 'ul': case 'ol': return '\n' + children;
                case 'blockquote': return '\n> ' + children.trim().replace(/\n/g, '\n> ') + '\n';
                case 'hr': return '\n---\n';
                case 'table': return '\n' + children + '\n';
                case 'tr': return children + '|\n';
                case 'th': return '| **' + children.trim() + '** ';
                case 'td': return '| ' + children.trim() + ' ';
                default: return children;
            }
        }

        const article = clone.querySelector('article, main, [role="main"]') || clone.querySelector('body') || clone.documentElement;
        let markdown = nodeToMarkdown(article);
        markdown = markdown.replace(/\n{3,}/g, '\n\n').trim();

        return {
            url: window.location.href,
            title: document.title || window.location.href,
            markdown,
        };
    })()
    "#;

    let result = cdpkit::runtime::methods::Evaluate::new(js)
        .with_return_by_value(true)
        .send(cdp, Some(session))
        .await?;

    let value = result
        .result
        .value
        .context("Failed to extract page markdown payload")?;
    let page: ExtractedPage =
        serde_json::from_value(value).context("Failed to parse extracted page payload")?;
    if page.markdown.trim().is_empty() {
        bail!("Failed to extract markdown from page");
    }
    Ok(page)
}

async fn reload_and_wait(cdp: &CDP, session: &str) -> Result<()> {
    cdpkit::page::methods::Enable::new()
        .send(cdp, Some(session))
        .await?;

    let mut events = cdpkit::page::events::LoadEventFired::subscribe(cdp);
    cdpkit::page::methods::Reload::new()
        .send(cdp, Some(session))
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(10), events.next())
        .await
        .context("Timed out waiting for page reload")?
        .context("Page load event stream closed unexpectedly")?;
    Ok(())
}

async fn snapshot_page_storage(cdp: &CDP, session: &str) -> Result<PageStorageSnapshot> {
    let js = r#"
    (() => {
        const dumpStorage = storage => {
            const out = {};
            for (let i = 0; i < storage.length; i += 1) {
                const key = storage.key(i);
                if (key !== null) {
                    out[key] = storage.getItem(key) ?? "";
                }
            }
            return out;
        };

        return {
            href: window.location.href,
            origin: window.location.origin,
            title: document.title,
            localStorage: dumpStorage(window.localStorage),
            sessionStorage: dumpStorage(window.sessionStorage),
        };
    })()
    "#;

    let result = cdpkit::runtime::methods::Evaluate::new(js)
        .with_return_by_value(true)
        .send(cdp, Some(session))
        .await?;

    let value = result
        .result
        .value
        .context("Failed to read page storage snapshot")?;
    let snapshot: PageStorageSnapshot =
        serde_json::from_value(value).context("Failed to parse page storage snapshot")?;

    if snapshot.origin == "null" {
        bail!(
            "Cannot save state for opaque origin '{}'; navigate to an http(s) page first",
            snapshot.href
        );
    }

    Ok(snapshot)
}

async fn apply_origin_storage_state(
    cdp: &CDP,
    session: &str,
    origin_state: &OriginStorageState,
) -> Result<()> {
    let payload = serde_json::to_string(origin_state)?;
    let js = format!(
        r#"
        (() => {{
            const state = {payload};
            if (window.location.origin !== state.origin) {{
                throw new Error(`Origin mismatch: expected ${{state.origin}}, got ${{window.location.origin}}`);
            }}

            window.localStorage.clear();
            for (const [key, value] of Object.entries(state.localStorage)) {{
                window.localStorage.setItem(key, value);
            }}

            window.sessionStorage.clear();
            for (const [key, value] of Object.entries(state.sessionStorage)) {{
                window.sessionStorage.setItem(key, value);
            }}

            return {{
                origin: window.location.origin,
                localStorage: window.localStorage.length,
                sessionStorage: window.sessionStorage.length,
            }};
        }})()
        "#
    );

    let result = cdpkit::runtime::methods::Evaluate::new(&js)
        .with_return_by_value(true)
        .send(cdp, Some(session))
        .await?;

    if result.exception_details.is_some() {
        bail!(
            "Failed to restore storage for origin '{}'",
            origin_state.origin
        );
    }

    Ok(())
}

/// Resolve a tab selector: numeric index or URL substring match.
fn resolve_tab<'a>(pages: &[&'a Tab], selector: &str) -> Result<&'a Tab> {
    if let Ok(index) = selector.parse::<usize>() {
        pages
            .get(index)
            .copied()
            .context(format!("No tab at index {}", index))
    } else {
        let matches: Vec<&&Tab> = pages
            .iter()
            .filter(|t| t.url.contains(selector) || t.title.contains(selector))
            .collect();
        match matches.len() {
            0 => bail!("No tab matching pattern '{}'", selector),
            1 => Ok(matches[0]),
            n => bail!(
                "Pattern '{}' matched {} tabs. Be more specific:\n{}",
                selector,
                n,
                matches
                    .iter()
                    .enumerate()
                    .map(|(i, t)| format!("  [{}] {} — {}", i, t.title, t.url))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        }
    }
}

/// Connect to a specific tab via cdpkit, creating a CDP session.
async fn connect_to_tab(cli: &Cli, selector: &str) -> Result<(CDP, String)> {
    let tabs = get_tabs(cli).await?;
    let pages: Vec<&Tab> = tabs.iter().filter(|t| t.tab_type == "page").collect();
    let tab = resolve_tab(&pages, selector)?;

    let cdp = CDP::connect(&format!("{}:{}", cli.host, cli.port))
        .await
        .context("Failed to connect CDP client")?;

    // Attach to the specific tab's target
    let attach = cdpkit::target::methods::AttachToTarget::new(&tab.id)
        .with_flatten(true)
        .send(&cdp, None)
        .await
        .context("Failed to attach to tab")?;

    Ok((cdp, attach.session_id))
}

async fn cmd_check(cli: &Cli) -> Result<()> {
    let resp = client(cli)
        .get(format!("{}/json/version", base_url(cli)))
        .send()
        .await
        .context(format!(
            "Browser not responding on {}:{}. Is remote debugging enabled?",
            cli.host, cli.port
        ))?;
    let version: BrowserVersion = resp.json().await?;
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&version)?);
    } else {
        println!(
            "OK — {} (protocol {})",
            version.browser, version.protocol_version
        );
    }
    Ok(())
}

async fn cmd_list(cli: &Cli) -> Result<()> {
    let tabs = get_tabs(cli).await?;
    let pages: Vec<&Tab> = tabs.iter().filter(|t| t.tab_type == "page").collect();
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&pages)?);
    } else {
        for (i, tab) in pages.iter().enumerate() {
            println!("[{}] {} — {}", i, tab.title, tab.url);
        }
    }
    Ok(())
}

async fn cmd_navigate(cli: &Cli, url: &str, tab_selector: &str) -> Result<()> {
    let (cdp, session) = connect_to_tab(cli, tab_selector).await?;
    cdpkit::page::methods::Enable::new()
        .send(&cdp, Some(&session))
        .await?;
    let mut events = cdpkit::page::events::LoadEventFired::subscribe(&cdp);
    let result = cdpkit::page::methods::Navigate::new(url)
        .send(&cdp, Some(&session))
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(10), events.next())
        .await
        .context(format!("Timed out waiting for '{}' to load", url))?
        .context("Page load event stream closed unexpectedly")?;

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "frameId": result.frame_id,
            }))?
        );
    } else {
        println!("Navigated to {}", url);
    }
    Ok(())
}

async fn cmd_evaluate(cli: &Cli, expression: &str, tab_selector: &str) -> Result<()> {
    let (cdp, session) = connect_to_tab(cli, tab_selector).await?;

    let result = cdpkit::runtime::methods::Evaluate::new(expression)
        .with_return_by_value(true)
        .send(&cdp, Some(&session))
        .await?;

    if cli.json {
        let json = serde_json::json!({
            "type": result.result.type_,
            "value": result.result.value,
            "description": result.result.description,
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else if let Some(value) = &result.result.value {
        match value {
            serde_json::Value::String(s) => println!("{}", s),
            other => println!("{}", other),
        }
    } else if let Some(desc) = &result.result.description {
        println!("{}", desc);
    }
    Ok(())
}

async fn cmd_screenshot(
    cli: &Cli,
    url: Option<&str>,
    output: Option<&str>,
    tab_selector: &str,
) -> Result<()> {
    let (cdp, session) = connect_to_tab(cli, tab_selector).await?;

    if let Some(url) = url {
        navigate_and_wait(&cdp, &session, url).await?;
    }

    let result = cdpkit::page::methods::CaptureScreenshot::new()
        .send(&cdp, Some(&session))
        .await?;

    if let Some(path) = output {
        let bytes = base64::engine::general_purpose::STANDARD.decode(&result.data)?;
        std::fs::write(path, bytes)?;
        eprintln!("Screenshot saved to {}", path);
    } else {
        println!("{}", result.data);
    }
    Ok(())
}

async fn cmd_markdown(cli: &Cli, url: &str, tab_selector: &str) -> Result<()> {
    let (cdp, session) = connect_to_tab(cli, tab_selector).await?;
    navigate_and_wait(&cdp, &session, url).await?;
    let page = extract_page_markdown(&cdp, &session).await?;
    println!("{}", page.markdown);
    Ok(())
}

async fn cmd_ingest(cli: &Cli, request: &IngestRequest<'_>) -> Result<()> {
    let (cdp, session) = connect_to_tab(cli, request.tab_selector).await?;
    navigate_and_wait(&cdp, &session, request.url).await?;
    let page = extract_page_markdown(&cdp, &session).await?;

    let subject = request
        .title_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(page.title.as_str())
        .to_string();
    let slug = resolve_ingest_slug(request.slug_override, &page.url, &subject)?;
    let labels = normalize_csv_values(request.labels_raw);
    let accounts = normalize_csv_values(request.accounts_raw);
    let thread_id = format!("web:{}", page.url);
    let timestamp = Utc::now().to_rfc2822();
    let sender = infer_ingest_sender(&page.url);
    let document = render_ingest_document(&IngestDocument {
        subject: &subject,
        labels: &labels,
        accounts: &accounts,
        thread_id: &thread_id,
        source_url: &page.url,
        source_title: &page.title,
        sender: &sender,
        timestamp: &timestamp,
        markdown: &page.markdown,
    });

    let corky_data_dir = resolve_corky_data_dir(request.corky_data_override)?;
    let conversations_dir = corky_data_dir.join("conversations");
    std::fs::create_dir_all(&conversations_dir).with_context(|| {
        format!(
            "Failed to create corky conversations dir {}",
            conversations_dir.display()
        )
    })?;
    let path = conversations_dir.join(format!("{slug}.md"));
    std::fs::write(&path, document)
        .with_context(|| format!("Failed to write {}", path.display()))?;

    if request.route {
        run_corky_command(&corky_data_dir, &["sync", "routes"])?;
    }
    if request.ragie_push {
        if request.ragie_full {
            run_corky_command(&corky_data_dir, &["ragie", "push", "--full"])?;
        } else {
            run_corky_command(&corky_data_dir, &["ragie", "push"])?;
        }
    }

    let output = IngestOutput {
        path: path.display().to_string(),
        corky_data_dir: corky_data_dir.display().to_string(),
        slug,
        subject,
        thread_id,
        source_url: page.url,
        routed: request.route,
        ragie_pushed: request.ragie_push,
    };

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Ingested {} into {}", output.source_url, output.path);
        println!("Subject: {}", output.subject);
        println!("Corky data dir: {}", output.corky_data_dir);
        if output.routed {
            println!("Post-step: ran `corky sync routes`");
        }
        if output.ragie_pushed {
            println!(
                "Post-step: ran `corky ragie push{}`",
                if request.ragie_full { " --full" } else { "" }
            );
        }
    }
    Ok(())
}

/// Resolve a CSS selector to DOM node coordinates, then click at center.
async fn cmd_click(cli: &Cli, selector: &str, tab_selector: &str) -> Result<()> {
    let (cdp, session) = connect_to_tab(cli, tab_selector).await?;

    // Get the document root node ID
    let doc = cdpkit::dom::methods::GetDocument::new()
        .send(&cdp, Some(&session))
        .await?;

    // Find the element by CSS selector
    let result = cdpkit::dom::methods::QuerySelector::new(doc.root.node_id, selector)
        .send(&cdp, Some(&session))
        .await
        .context(format!("No element matching selector '{}'", selector))?;

    if result.node_id == 0 {
        bail!("No element matching selector '{}'", selector);
    }

    // Get the element's box model for coordinates
    let box_model = cdpkit::dom::methods::GetBoxModel::new()
        .with_node_id(result.node_id)
        .send(&cdp, Some(&session))
        .await
        .context("Failed to get element box model")?;

    // Calculate center of the content quad: [x1,y1, x2,y2, x3,y3, x4,y4]
    let q = &box_model.model.content;
    let cx = (q[0] + q[2] + q[4] + q[6]) / 4.0;
    let cy = (q[1] + q[3] + q[5] + q[7]) / 4.0;

    // Dispatch mouse events: move, press, release
    cdpkit::input::methods::DispatchMouseEvent::new("mouseMoved", cx, cy)
        .send(&cdp, Some(&session))
        .await?;
    cdpkit::input::methods::DispatchMouseEvent::new("mousePressed", cx, cy)
        .with_button(cdpkit::input::types::MouseButton::Left)
        .with_click_count(1)
        .send(&cdp, Some(&session))
        .await?;
    cdpkit::input::methods::DispatchMouseEvent::new("mouseReleased", cx, cy)
        .with_button(cdpkit::input::types::MouseButton::Left)
        .with_click_count(1)
        .send(&cdp, Some(&session))
        .await?;

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "selector": selector,
                "x": cx,
                "y": cy,
            }))?
        );
    } else {
        println!("Clicked '{}' at ({:.0}, {:.0})", selector, cx, cy);
    }
    Ok(())
}

/// Focus an element and type text into it, with paragraph handling for contenteditable.
async fn cmd_type(cli: &Cli, selector: &str, text: &str, tab_selector: &str) -> Result<()> {
    let (cdp, session) = connect_to_tab(cli, tab_selector).await?;

    // Get document root
    let doc = cdpkit::dom::methods::GetDocument::new()
        .send(&cdp, Some(&session))
        .await?;

    // Find and focus the target element
    let result = cdpkit::dom::methods::QuerySelector::new(doc.root.node_id, selector)
        .send(&cdp, Some(&session))
        .await
        .context(format!("No element matching selector '{}'", selector))?;

    if result.node_id == 0 {
        bail!("No element matching selector '{}'", selector);
    }

    cdpkit::dom::methods::Focus::new()
        .with_node_id(result.node_id)
        .send(&cdp, Some(&session))
        .await
        .context("Failed to focus element")?;

    // Split text on double-newlines for paragraph handling.
    // Between paragraphs, send Shift+Enter (line break without submit).
    let paragraphs: Vec<&str> = text.split("\n\n").collect();

    for (i, paragraph) in paragraphs.iter().enumerate() {
        if !paragraph.is_empty() {
            cdpkit::input::methods::InsertText::new(*paragraph)
                .send(&cdp, Some(&session))
                .await?;
        }

        if i < paragraphs.len() - 1 {
            // Shift+Enter for line break (modifier 8 = Shift)
            cdpkit::input::methods::DispatchKeyEvent::new("keyDown")
                .with_key("Enter")
                .with_code("Enter")
                .with_text("\r")
                .with_modifiers(8)
                .with_windows_virtual_key_code(13)
                .send(&cdp, Some(&session))
                .await?;
            cdpkit::input::methods::DispatchKeyEvent::new("keyUp")
                .with_key("Enter")
                .with_code("Enter")
                .with_modifiers(8)
                .with_windows_virtual_key_code(13)
                .send(&cdp, Some(&session))
                .await?;
            // Second Shift+Enter for visual paragraph gap
            cdpkit::input::methods::DispatchKeyEvent::new("keyDown")
                .with_key("Enter")
                .with_code("Enter")
                .with_text("\r")
                .with_modifiers(8)
                .with_windows_virtual_key_code(13)
                .send(&cdp, Some(&session))
                .await?;
            cdpkit::input::methods::DispatchKeyEvent::new("keyUp")
                .with_key("Enter")
                .with_code("Enter")
                .with_modifiers(8)
                .with_windows_virtual_key_code(13)
                .send(&cdp, Some(&session))
                .await?;
        }
    }

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "selector": selector,
                "length": text.len(),
                "paragraphs": paragraphs.len(),
            }))?
        );
    } else {
        println!(
            "Typed {} chars ({} paragraph{}) into '{}'",
            text.len(),
            paragraphs.len(),
            if paragraphs.len() == 1 { "" } else { "s" },
            selector
        );
    }
    Ok(())
}

/// Activate a browser tab by bringing it to the foreground.
async fn cmd_select_tab(cli: &Cli, selector: &str) -> Result<()> {
    let tabs = get_tabs(cli).await?;
    let pages: Vec<&Tab> = tabs.iter().filter(|t| t.tab_type == "page").collect();
    let tab = resolve_tab(&pages, selector)?;

    // Use the HTTP endpoint to activate the tab
    let resp = client(cli)
        .get(format!("{}/json/activate/{}", base_url(cli), tab.id))
        .send()
        .await
        .context("Failed to activate tab")?;

    if !resp.status().is_success() {
        bail!("Failed to activate tab: HTTP {}", resp.status());
    }

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "id": tab.id,
                "title": tab.title,
                "url": tab.url,
            }))?
        );
    } else {
        println!("Activated: {} — {}", tab.title, tab.url);
    }
    Ok(())
}

/// Wait for a CSS selector to appear in the DOM by polling.
async fn cmd_wait(cli: &Cli, selector: &str, timeout_ms: u64, tab_selector: &str) -> Result<()> {
    let (cdp, session) = connect_to_tab(cli, tab_selector).await?;

    let js = format!(
        r#"document.querySelector({}) !== null"#,
        serde_json::to_string(selector)?
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let poll_interval = std::time::Duration::from_millis(250);

    loop {
        let result = cdpkit::runtime::methods::Evaluate::new(&js)
            .with_return_by_value(true)
            .send(&cdp, Some(&session))
            .await?;

        if result.result.value == Some(serde_json::Value::Bool(true)) {
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "selector": selector,
                        "found": true,
                    }))?
                );
            } else {
                println!("Found '{}'", selector);
            }
            return Ok(());
        }

        if std::time::Instant::now() >= deadline {
            bail!(
                "Timeout waiting for selector '{}' after {}ms",
                selector,
                timeout_ms
            );
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// Dump the accessibility tree for the page.
async fn cmd_snapshot(cli: &Cli, depth: Option<i64>, raw: bool, tab_selector: &str) -> Result<()> {
    let (cdp, session) = connect_to_tab(cli, tab_selector).await?;

    let mut req = cdpkit::accessibility::methods::GetFullAxTree::new();
    if let Some(d) = depth {
        req = req.with_depth(d);
    }

    let result = req.send(&cdp, Some(&session)).await?;

    if cli.json {
        if raw {
            println!("{}", serde_json::to_string_pretty(&result.nodes)?);
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&build_snapshot_output(&result.nodes))?
            );
        }
    } else {
        println!("{}", render_snapshot_text(&result.nodes));
    }
    Ok(())
}

async fn cmd_network_list(
    cli: &Cli,
    url: Option<&str>,
    capture_timeout_ms: u64,
    idle_ms: u64,
    limit: Option<usize>,
    tab_selector: &str,
) -> Result<()> {
    let (trigger, captured, _cdp, _session) =
        capture_network_requests(cli, url, capture_timeout_ms, idle_ms, tab_selector).await?;
    let requests = captured
        .iter()
        .map(network_summary)
        .collect::<Vec<NetworkRequestSummary>>();

    let output = NetworkListOutput {
        trigger,
        request_count: requests.len(),
        requests,
    };

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!(
            "Captured {} request{} via {}",
            output.request_count,
            if output.request_count == 1 { "" } else { "s" },
            output.trigger
        );
        for request in output.requests.iter().take(limit.unwrap_or(usize::MAX)) {
            let status = if request.failed {
                "failed".to_string()
            } else if let Some(status) = request.status {
                status.to_string()
            } else if request.finished {
                "done".to_string()
            } else {
                "pending".to_string()
            };
            let resource_type = request.resource_type.as_deref().unwrap_or("unknown");
            let mime = request.mime_type.as_deref().unwrap_or("?");
            println!(
                "[{status} {resource_type}] {} {}",
                request.method, request.url
            );
            println!(
                "  id={} size={} mime={}",
                request.request_id,
                format_size(request.encoded_data_length),
                mime
            );
        }
    }

    Ok(())
}

async fn cmd_network_inspect(
    cli: &Cli,
    matcher: &str,
    url: Option<&str>,
    capture_timeout_ms: u64,
    idle_ms: u64,
    body_limit: usize,
    tab_selector: &str,
) -> Result<()> {
    let (trigger, captured, cdp, session) =
        capture_network_requests(cli, url, capture_timeout_ms, idle_ms, tab_selector).await?;
    let (matched, matched_by, matched_count) = select_network_request(&captured, matcher)?;

    let request_post_data = if let Some(post_data) = matched.post_data.clone() {
        Some(post_data)
    } else if matched.has_post_data {
        match cdpkit::network::methods::GetRequestPostData::new(matched.request_id.clone())
            .send(&cdp, Some(&session))
            .await
        {
            Ok(result) => Some(result.post_data),
            Err(_) => None,
        }
    } else {
        None
    };

    let request_post_data_error = if matched.has_post_data && request_post_data.is_none() {
        Some("CDP did not retain request post data for this request".to_string())
    } else {
        None
    };

    let (response_body, response_body_error) = if matched.response_status.is_some() {
        match cdpkit::network::methods::GetResponseBody::new(matched.request_id.clone())
            .send(&cdp, Some(&session))
            .await
        {
            Ok(result) => (
                Some(decode_network_body(
                    &result.body,
                    result.base64_encoded,
                    matched.mime_type.as_deref(),
                    body_limit,
                )),
                None,
            ),
            Err(err) => (None, Some(err.to_string())),
        }
    } else {
        (
            None,
            Some("No HTTP response was captured for this request".to_string()),
        )
    };

    let output = NetworkInspectOutput {
        trigger,
        matcher: matcher.to_string(),
        matched_by: match matched_by {
            NetworkMatchKind::RequestId => "requestId".to_string(),
            NetworkMatchKind::UrlPattern => "urlPattern".to_string(),
        },
        matched_count,
        request: network_summary(matched),
        request_headers: matched.request_headers.clone(),
        response_headers: matched.response_headers.clone(),
        request_post_data,
        request_post_data_error,
        response_body,
        response_body_error,
    };

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Trigger: {}", output.trigger);
        println!(
            "Matched by {} ({} match{})",
            output.matched_by,
            output.matched_count,
            if output.matched_count == 1 { "" } else { "es" }
        );
        println!("Request id: {}", output.request.request_id);
        println!(
            "Type: {}",
            output.request.resource_type.as_deref().unwrap_or("unknown")
        );
        println!("Method: {}", output.request.method);
        println!("URL: {}", output.request.url);
        println!(
            "Status: {}",
            output
                .request
                .status
                .map(|status| {
                    if let Some(text) = output.request.status_text.as_deref() {
                        format!("{status} {text}")
                    } else {
                        status.to_string()
                    }
                })
                .unwrap_or_else(|| "n/a".to_string())
        );
        println!(
            "Mime: {}",
            output.request.mime_type.as_deref().unwrap_or("unknown")
        );
        println!(
            "Protocol: {}",
            output.request.protocol.as_deref().unwrap_or("unknown")
        );
        if let Some(remote_ip) = output.request.remote_ip_address.as_deref() {
            if let Some(remote_port) = output.request.remote_port {
                println!("Remote: {remote_ip}:{remote_port}");
            } else {
                println!("Remote: {remote_ip}");
            }
        }
        println!(
            "Cache: disk={} service_worker={}",
            output.request.from_disk_cache, output.request.from_service_worker
        );
        println!(
            "Encoded size: {}",
            format_size(output.request.encoded_data_length)
        );
        if output.request.failed {
            println!(
                "Failure: {}",
                output
                    .request
                    .error_text
                    .as_deref()
                    .unwrap_or("unknown error")
            );
        }
        print_json_block("Request headers", &output.request_headers)?;
        if let Some(response_headers) = output.response_headers.as_ref() {
            print_json_block("Response headers", response_headers)?;
        }
        if let Some(post_data) = output.request_post_data.as_deref() {
            println!("Request body:");
            println!("{post_data}");
        } else if let Some(err) = output.request_post_data_error.as_deref() {
            println!("Request body: {err}");
        }
        if let Some(body) = output.response_body.as_ref() {
            println!(
                "Response body (encoding={}, truncated={}):",
                body.encoding, body.truncated
            );
            println!("{}", body.content);
        } else if let Some(err) = output.response_body_error.as_deref() {
            println!("Response body: {err}");
        }
    }

    Ok(())
}

async fn cmd_state_save(
    cli: &Cli,
    name: &str,
    explicit_path: Option<&str>,
    tab_selector: &str,
) -> Result<()> {
    let path = resolve_state_path(name, explicit_path)?;
    let (cdp, session) = connect_to_tab(cli, tab_selector).await?;
    let page = snapshot_page_storage(&cdp, &session).await?;
    let cookies = cdpkit::network::methods::GetCookies::new()
        .with_urls(vec![page.href.clone()])
        .send(&cdp, Some(&session))
        .await?
        .cookies
        .into_iter()
        .map(cookie_to_param)
        .collect::<Vec<_>>();

    let state = SavedBrowserState {
        version: 1,
        saved_at_unix_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("System clock is before UNIX_EPOCH")?
            .as_millis()
            .try_into()
            .context("Timestamp overflow while saving state")?,
        active_url: page.href.clone(),
        active_title: page.title.clone(),
        cookies,
        origins: vec![OriginStorageState {
            origin: page.origin,
            url: page.href,
            title: page.title,
            local_storage: page.local_storage,
            session_storage: page.session_storage,
        }],
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&state)?)?;

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "name": name,
                "path": path.display().to_string(),
                "cookies": state.cookies.len(),
                "origins": state.origins.len(),
                "activeUrl": state.active_url,
            }))?
        );
    } else {
        println!(
            "Saved state '{}' to {} ({} cookies, {} origin{})",
            name,
            path.display(),
            state.cookies.len(),
            state.origins.len(),
            if state.origins.len() == 1 { "" } else { "s" }
        );
    }
    Ok(())
}

async fn cmd_state_load(
    cli: &Cli,
    name: &str,
    explicit_path: Option<&str>,
    tab_selector: &str,
) -> Result<()> {
    let path = resolve_state_path(name, explicit_path)?;
    let state: SavedBrowserState = serde_json::from_str(
        &std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read state file {}", path.display()))?,
    )
    .with_context(|| format!("Failed to parse state file {}", path.display()))?;

    if state.version != 1 {
        bail!(
            "Unsupported state snapshot version {} in {}",
            state.version,
            path.display()
        );
    }

    let (cdp, session) = connect_to_tab(cli, tab_selector).await?;

    if !state.cookies.is_empty() {
        cdpkit::network::methods::SetCookies::new(state.cookies.clone())
            .send(&cdp, Some(&session))
            .await?;
    }

    for origin_state in &state.origins {
        navigate_and_wait(&cdp, &session, &origin_state.url).await?;
        apply_origin_storage_state(&cdp, &session, origin_state).await?;
        reload_and_wait(&cdp, &session).await?;
    }

    if state.origins.last().map(|origin| origin.url.as_str()) != Some(state.active_url.as_str()) {
        navigate_and_wait(&cdp, &session, &state.active_url).await?;
    }

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "name": name,
                "path": path.display().to_string(),
                "cookies": state.cookies.len(),
                "origins": state.origins.len(),
                "activeUrl": state.active_url,
            }))?
        );
    } else {
        println!(
            "Loaded state '{}' from {} ({} cookies, {} origin{})",
            name,
            path.display(),
            state.cookies.len(),
            state.origins.len(),
            if state.origins.len() == 1 { "" } else { "s" }
        );
    }
    Ok(())
}

fn cmd_state_list(cli: &Cli) -> Result<()> {
    let root = state_root()?;
    let mut entries = Vec::new();

    if root.exists() {
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }

            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| path.display().to_string());
            entries.push(StateListEntry {
                name,
                path: path.display().to_string(),
            });
        }
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else if entries.is_empty() {
        println!("No saved states in {}", root.display());
    } else {
        for entry in entries {
            println!("{} — {}", entry.name, entry.path);
        }
    }
    Ok(())
}

/// The SKILL.md content bundled at build time.
const BUNDLED_SKILL: &str = include_str!("../SKILL.md");

/// Resolve the project root for skill installation.
fn resolve_skill_root() -> std::path::PathBuf {
    // Try git superproject first (handles submodule CWD)
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--show-superproject-working-tree"])
        .output()
    {
        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !root.is_empty() {
            return std::path::PathBuf::from(root);
        }
    }
    // Fall back to git toplevel
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
    {
        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !root.is_empty() {
            return std::path::PathBuf::from(root);
        }
    }
    // Fall back to CWD
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

fn cmd_skill_install(cli: &Cli) -> Result<()> {
    let root = resolve_skill_root();
    let dir = root.join(".claude/skills/chromium-bridge");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("SKILL.md");

    let already_current = path.exists()
        && std::fs::read_to_string(&path)
            .map(|existing| existing == BUNDLED_SKILL)
            .unwrap_or(false);

    if already_current {
        if cli.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "path": path.display().to_string(),
                    "updated": false,
                }))?
            );
        } else {
            println!("Skill already up to date: {}", path.display());
        }
    } else {
        std::fs::write(&path, BUNDLED_SKILL)?;
        if cli.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "path": path.display().to_string(),
                    "updated": true,
                }))?
            );
        } else {
            println!("Skill installed: {}", path.display());
        }
    }
    Ok(())
}

fn cmd_skill_check(cli: &Cli) -> Result<()> {
    let root = resolve_skill_root();
    let path = root.join(".claude/skills/chromium-bridge/SKILL.md");

    let up_to_date = path.exists()
        && std::fs::read_to_string(&path)
            .map(|existing| existing == BUNDLED_SKILL)
            .unwrap_or(false);

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "path": path.display().to_string(),
                "up_to_date": up_to_date,
            }))?
        );
    } else if up_to_date {
        println!("Skill up to date: {}", path.display());
    } else if path.exists() {
        eprintln!("Skill outdated: {}", path.display());
        eprintln!("Run: chromium-bridge skill install");
        std::process::exit(1);
    } else {
        eprintln!("Skill not installed");
        eprintln!("Run: chromium-bridge skill install");
        std::process::exit(1);
    }
    Ok(())
}

fn cmd_setup() -> Result<()> {
    let browsers = [
        (
            "Brave",
            "/opt/brave-bin/brave",
            "~/.config/brave-flags.conf",
        ),
        (
            "Chrome",
            "/usr/bin/google-chrome-stable",
            "~/.config/chrome-flags.conf",
        ),
        (
            "Chromium",
            "/usr/bin/chromium",
            "~/.config/chromium-flags.conf",
        ),
    ];

    println!("Detected browsers:");
    let mut found = false;
    for (name, path, flags_file) in &browsers {
        if std::path::Path::new(path).exists() {
            found = true;
            let flags_path = flags_file.replace("~", &std::env::var("HOME").unwrap_or_default());
            let has_flag = std::fs::read_to_string(&flags_path)
                .map(|c| c.contains("--remote-debugging-port"))
                .unwrap_or(false);
            let status = if has_flag {
                "remote debugging configured"
            } else {
                "remote debugging NOT configured"
            };
            println!("  [{}] {} — {}", name, path, status);
            if !has_flag {
                println!(
                    "    → echo \"--remote-debugging-port=9222\" >> {}",
                    flags_file
                );
            }
        }
    }
    if !found {
        println!("  No Chromium-based browsers found.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_state_paths_live_under_config_dir() {
        let root = state_root_from_env(
            Some("/tmp/chromium-bridge-state-tests"),
            None,
            Some("/tmp/home"),
        )
        .expect("state root");
        let path = root.join("linkedin-auth.json");
        assert_eq!(
            path,
            PathBuf::from("/tmp/chromium-bridge-state-tests/linkedin-auth.json")
        );
    }

    #[test]
    fn explicit_state_path_expands_home() {
        let path = expand_path_with_home("~/saved/state.json", Some("/tmp/chromium-bridge-home"))
            .expect("state path");
        assert_eq!(
            path,
            PathBuf::from("/tmp/chromium-bridge-home/saved/state.json")
        );
    }

    #[test]
    fn named_state_rejects_path_separators() {
        let err = resolve_state_path("nested/state", None).expect_err("expected invalid name");
        assert!(
            err.to_string()
                .contains("Snapshot names cannot contain path separators")
        );
    }

    #[test]
    fn network_request_match_prefers_exact_request_id() {
        let requests = vec![
            CapturedNetworkRequest {
                request_id: "1234.1".to_string(),
                document_url: "https://example.com".to_string(),
                url: "https://example.com/api/search".to_string(),
                method: "GET".to_string(),
                resource_type: Some("XHR".to_string()),
                initiator_type: "script".to_string(),
                initiator_url: None,
                request_headers: serde_json::json!({}),
                has_post_data: false,
                post_data: None,
                response_status: Some(200),
                response_status_text: Some("OK".to_string()),
                response_headers: Some(serde_json::json!({})),
                mime_type: Some("application/json".to_string()),
                protocol: Some("h2".to_string()),
                remote_ip_address: None,
                remote_port: None,
                from_disk_cache: false,
                from_service_worker: false,
                encoded_data_length: Some(42.0),
                failed: false,
                canceled: false,
                error_text: None,
                finished: true,
            },
            CapturedNetworkRequest {
                request_id: "5678.9".to_string(),
                document_url: "https://example.com".to_string(),
                url: "https://example.com/api/other".to_string(),
                method: "GET".to_string(),
                resource_type: Some("XHR".to_string()),
                initiator_type: "script".to_string(),
                initiator_url: None,
                request_headers: serde_json::json!({}),
                has_post_data: false,
                post_data: None,
                response_status: Some(200),
                response_status_text: Some("OK".to_string()),
                response_headers: Some(serde_json::json!({})),
                mime_type: Some("application/json".to_string()),
                protocol: Some("h2".to_string()),
                remote_ip_address: None,
                remote_port: None,
                from_disk_cache: false,
                from_service_worker: false,
                encoded_data_length: Some(42.0),
                failed: false,
                canceled: false,
                error_text: None,
                finished: true,
            },
        ];

        let (matched, matched_by, matched_count) =
            select_network_request(&requests, "1234.1").expect("matched request");
        assert_eq!(matched.request_id, "1234.1");
        assert!(matches!(matched_by, NetworkMatchKind::RequestId));
        assert_eq!(matched_count, 1);
    }

    #[test]
    fn decode_network_body_decodes_utf8_from_base64() {
        let encoded = base64::engine::general_purpose::STANDARD.encode("{\"ok\":true}");
        let body = decode_network_body(&encoded, true, Some("application/json"), 4000);
        assert_eq!(body.content, "{\"ok\":true}");
        assert_eq!(body.encoding, "utf8");
        assert!(body.was_base64_encoded);
        assert!(!body.truncated);
    }

    #[test]
    fn corky_data_resolution_matches_corky_defaults() {
        let cwd = PathBuf::from("/tmp/workspace");
        let resolved =
            resolve_corky_data_dir_from(None, &cwd, Some("/tmp/mail-root"), Some("/tmp/home"))
                .expect("resolved dir");
        assert_eq!(resolved, PathBuf::from("/tmp/mail-root"));

        let fallback =
            resolve_corky_data_dir_from(None, &cwd, None, Some("/tmp/home")).expect("fallback");
        assert_eq!(fallback, PathBuf::from("/tmp/home/Documents/mail"));
    }

    #[test]
    fn ingest_slug_defaults_to_host_and_path() {
        let slug = resolve_ingest_slug(
            None,
            "https://docs.example.com/guides/intro/",
            "Ignored Title",
        )
        .expect("slug");
        assert_eq!(slug, "docs-example-com-guides-intro");
    }

    #[test]
    fn ingest_document_uses_corky_thread_shape() {
        let rendered = render_ingest_document(&IngestDocument {
            subject: "Example Subject",
            labels: &[String::from("web"), String::from("research")],
            accounts: &[String::from("chromium-bridge")],
            thread_id: "web:https://example.com/guide",
            source_url: "https://example.com/guide",
            source_title: "Example Page",
            sender: "example.com",
            timestamp: "Tue, 05 May 2026 12:00:00 +0000",
            markdown: "# Heading\n\nBody text",
        });
        assert!(rendered.contains("**Labels**: web, research"));
        assert!(rendered.contains("**Thread ID**: web:https://example.com/guide"));
        assert!(rendered.contains("**Source URL**: https://example.com/guide"));
        assert!(rendered.contains("## example.com — Tue, 05 May 2026 12:00:00 +0000"));
        assert!(rendered.contains("# Heading\n\nBody text"));
    }

    fn ax_string(value: &str) -> cdpkit::accessibility::types::AXValue {
        cdpkit::accessibility::types::AXValue {
            type_: cdpkit::accessibility::types::AXValueType::String,
            value: Some(serde_json::Value::String(value.to_string())),
            related_nodes: None,
            sources: None,
        }
    }

    fn ax_bool(value: bool) -> cdpkit::accessibility::types::AXValue {
        cdpkit::accessibility::types::AXValue {
            type_: cdpkit::accessibility::types::AXValueType::Boolean,
            value: Some(serde_json::Value::Bool(value)),
            related_nodes: None,
            sources: None,
        }
    }

    #[test]
    fn snapshot_output_prefers_backend_dom_refs_and_preserves_relationships() {
        let nodes = vec![
            cdpkit::accessibility::types::AXNode {
                node_id: "root".to_string(),
                ignored: false,
                ignored_reasons: None,
                role: Some(ax_string("RootWebArea")),
                chrome_role: None,
                name: Some(ax_string("Inbox")),
                description: None,
                value: None,
                properties: None,
                parent_id: None,
                child_ids: Some(vec!["child".to_string()]),
                backend_dom_node_id: Some(101),
                frame_id: None,
            },
            cdpkit::accessibility::types::AXNode {
                node_id: "child".to_string(),
                ignored: false,
                ignored_reasons: None,
                role: Some(ax_string("button")),
                chrome_role: None,
                name: Some(ax_string("Compose")),
                description: None,
                value: None,
                properties: Some(vec![cdpkit::accessibility::types::AXProperty {
                    name: cdpkit::accessibility::types::AXPropertyName::Focused,
                    value: ax_bool(true),
                }]),
                parent_id: Some("root".to_string()),
                child_ids: None,
                backend_dom_node_id: Some(202),
                frame_id: None,
            },
        ];

        let snapshot = build_snapshot_output(&nodes);
        assert_eq!(snapshot.root_refs, vec!["dom:101"]);
        assert_eq!(snapshot.nodes[0].child_refs, vec!["dom:202"]);
        assert_eq!(snapshot.nodes[1].parent_ref.as_deref(), Some("dom:101"));
        assert_eq!(snapshot.nodes[1].ref_id, "dom:202");
        assert_eq!(
            snapshot.nodes[1].properties["focused"].value,
            Some(serde_json::Value::Bool(true))
        );
    }

    #[test]
    fn snapshot_text_skips_generic_wrappers_and_keeps_refs() {
        let nodes = vec![
            cdpkit::accessibility::types::AXNode {
                node_id: "root".to_string(),
                ignored: false,
                ignored_reasons: None,
                role: Some(ax_string("RootWebArea")),
                chrome_role: None,
                name: Some(ax_string("Inbox")),
                description: None,
                value: None,
                properties: None,
                parent_id: None,
                child_ids: Some(vec!["wrapper".to_string()]),
                backend_dom_node_id: Some(101),
                frame_id: None,
            },
            cdpkit::accessibility::types::AXNode {
                node_id: "wrapper".to_string(),
                ignored: false,
                ignored_reasons: None,
                role: Some(ax_string("generic")),
                chrome_role: None,
                name: None,
                description: None,
                value: None,
                properties: None,
                parent_id: Some("root".to_string()),
                child_ids: Some(vec!["child".to_string()]),
                backend_dom_node_id: Some(202),
                frame_id: None,
            },
            cdpkit::accessibility::types::AXNode {
                node_id: "child".to_string(),
                ignored: false,
                ignored_reasons: None,
                role: Some(ax_string("button")),
                chrome_role: None,
                name: Some(ax_string("Compose")),
                description: None,
                value: None,
                properties: None,
                parent_id: Some("wrapper".to_string()),
                child_ids: None,
                backend_dom_node_id: Some(303),
                frame_id: None,
            },
        ];

        let rendered = render_snapshot_text(&nodes);
        assert_eq!(
            rendered,
            "[RootWebArea] Inbox (ref=dom:101)\n  [button] Compose (ref=dom:303)"
        );
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Check => cmd_check(&cli).await,
        Command::List => cmd_list(&cli).await,
        Command::Navigate { url, tab } => cmd_navigate(&cli, url, tab).await,
        Command::Evaluate { expression, tab } => cmd_evaluate(&cli, expression, tab).await,
        Command::Screenshot { url, output, tab } => {
            cmd_screenshot(&cli, url.as_deref(), output.as_deref(), tab).await
        }
        Command::Markdown { url, tab } => cmd_markdown(&cli, url, tab).await,
        Command::Click { selector, tab } => cmd_click(&cli, selector, tab).await,
        Command::Type {
            selector,
            text,
            tab,
        } => cmd_type(&cli, selector, text, tab).await,
        Command::SelectTab { selector } => cmd_select_tab(&cli, selector).await,
        Command::Wait {
            selector,
            wait_timeout,
            tab,
        } => cmd_wait(&cli, selector, *wait_timeout, tab).await,
        Command::Snapshot { depth, raw, tab } => cmd_snapshot(&cli, *depth, *raw, tab).await,
        Command::Network { action } => match action {
            NetworkAction::List {
                url,
                capture_timeout,
                idle_ms,
                limit,
                tab,
            } => {
                cmd_network_list(
                    &cli,
                    url.as_deref(),
                    *capture_timeout,
                    *idle_ms,
                    *limit,
                    tab,
                )
                .await
            }
            NetworkAction::Inspect {
                matcher,
                url,
                capture_timeout,
                idle_ms,
                body_limit,
                tab,
            } => {
                cmd_network_inspect(
                    &cli,
                    matcher,
                    url.as_deref(),
                    *capture_timeout,
                    *idle_ms,
                    *body_limit,
                    tab,
                )
                .await
            }
        },
        Command::Ingest {
            url,
            title,
            slug,
            labels,
            accounts,
            corky_data,
            route,
            ragie_push,
            ragie_full,
            tab,
        } => {
            cmd_ingest(
                &cli,
                &IngestRequest {
                    url,
                    title_override: title.as_deref(),
                    slug_override: slug.as_deref(),
                    labels_raw: labels,
                    accounts_raw: accounts,
                    corky_data_override: corky_data.as_deref(),
                    route: *route,
                    ragie_push: *ragie_push,
                    ragie_full: *ragie_full,
                    tab_selector: tab,
                },
            )
            .await
        }
        Command::State { action } => match action {
            StateAction::Save { name, path, tab } => {
                cmd_state_save(&cli, name, path.as_deref(), tab).await
            }
            StateAction::Load { name, path, tab } => {
                cmd_state_load(&cli, name, path.as_deref(), tab).await
            }
            StateAction::List => cmd_state_list(&cli),
        },
        Command::Skill { action } => match action {
            SkillAction::Install => cmd_skill_install(&cli),
            SkillAction::Check => cmd_skill_check(&cli),
        },
        Command::Setup => cmd_setup(),
    }
}
