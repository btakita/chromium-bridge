//! # Module: main (chromium-bridge CLI)
//!
//! ## Spec
//! - Entry point for the `chromium-bridge` binary; parses CLI with `clap` derive.
//! - `Cli` struct holds global options (`--host`, `--port`, `--timeout`, `--json`)
//!   and a `Command` subcommand enum.
//! - Commands: `Check`, `List`, `Navigate`, `Evaluate`, `Screenshot`, `Markdown`, `Setup`,
//!   `Click`, `Type`, `SelectTab`, `Wait`, `Snapshot`.
//! - CDP communication: HTTP (`/json/*`) for tab listing/version, WebSocket via `cdpkit`
//!   for page-level commands (navigate, evaluate, screenshot, input).
//! - `connect_to_tab` creates a cdpkit `CDP` client and attaches to a specific tab by index.
//! - Screenshot and markdown commands use `LoadEventFired` event streaming for page load
//!   detection instead of fixed delays.
//! - `cmd_markdown` injects a DOM walker JS that converts page content to clean markdown.
//! - `cmd_setup` detects installed Chromium browsers and checks debugging flag status.
//!
//! ## Agentic Contracts
//! - All commands return `anyhow::Result<()>`; errors propagate to stderr.
//! - `--json` flag produces machine-readable output on all commands; human-readable by default.
//! - `--tab` accepts an index (e.g., `0`) or URL/title substring pattern (e.g., `linkedin`).
//!   Ambiguous patterns (multiple matches) produce an error listing matches.
//! - CDP connection timeout is configurable via `--timeout` (default 5000ms).
//! - Env vars `CHROMIUM_BRIDGE_HOST` and `CHROMIUM_BRIDGE_PORT` override defaults.
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
//! - snapshot_ax_tree: `chromium-bridge snapshot` → prints accessibility tree

use anyhow::{Context, Result, bail};
use base64::Engine;
use cdpkit::CDP;
use clap::{Parser, Subcommand};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[command(name = "chromium-bridge", about = "Bridge agents to Chromium browsers via CDP")]
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
        /// Target tab: index number or URL substring pattern (default: 0)
        #[arg(long, default_value = "0")]
        tab: String,
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

    // Enable page domain for load events
    cdpkit::page::methods::Enable::new()
        .send(&cdp, Some(&session))
        .await?;

    let result = cdpkit::page::methods::Navigate::new(url)
        .send(&cdp, Some(&session))
        .await?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "frameId": result.frame_id,
        }))?);
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
        cdpkit::page::methods::Enable::new()
            .send(&cdp, Some(&session))
            .await?;

        cdpkit::page::methods::Navigate::new(url)
            .send(&cdp, Some(&session))
            .await?;

        // Wait for page load event
        let mut events = cdpkit::page::events::LoadEventFired::subscribe(&cdp);
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            events.next(),
        )
        .await;
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

    cdpkit::page::methods::Enable::new()
        .send(&cdp, Some(&session))
        .await?;

    cdpkit::page::methods::Navigate::new(url)
        .send(&cdp, Some(&session))
        .await?;

    // Wait for page load event instead of fixed sleep
    let mut events = cdpkit::page::events::LoadEventFired::subscribe(&cdp);
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        events.next(),
    )
    .await;

    let js = r#"
    (function() {
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
        let md = nodeToMarkdown(article);
        md = md.replace(/\n{3,}/g, '\n\n').trim();
        return md;
    })()
    "#;

    let result = cdpkit::runtime::methods::Evaluate::new(js)
        .with_return_by_value(true)
        .send(&cdp, Some(&session))
        .await?;

    if let Some(serde_json::Value::String(md)) = &result.result.value {
        println!("{}", md);
    } else {
        bail!("Failed to extract markdown from page");
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
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "selector": selector,
            "x": cx,
            "y": cy,
        }))?);
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
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "selector": selector,
            "length": text.len(),
            "paragraphs": paragraphs.len(),
        }))?);
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
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "id": tab.id,
            "title": tab.title,
            "url": tab.url,
        }))?);
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
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "selector": selector,
                    "found": true,
                }))?);
            } else {
                println!("Found '{}'", selector);
            }
            return Ok(());
        }

        if std::time::Instant::now() >= deadline {
            bail!("Timeout waiting for selector '{}' after {}ms", selector, timeout_ms);
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// Dump the accessibility tree for the page.
async fn cmd_snapshot(cli: &Cli, depth: Option<i64>, tab_selector: &str) -> Result<()> {
    let (cdp, session) = connect_to_tab(cli, tab_selector).await?;

    let mut req = cdpkit::accessibility::methods::GetFullAxTree::new();
    if let Some(d) = depth {
        req = req.with_depth(d);
    }

    let result = req.send(&cdp, Some(&session)).await?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&result.nodes)?);
    } else {
        // Build a compact human-readable tree
        for node in &result.nodes {
            if node.ignored {
                continue;
            }
            let role = node
                .role
                .as_ref()
                .and_then(|v| v.value.as_ref())
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let name = node
                .name
                .as_ref()
                .and_then(|v| v.value.as_ref())
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if role == "none" || role == "generic" {
                continue;
            }

            if name.is_empty() {
                println!("[{}]", role);
            } else {
                let truncated = if name.len() > 80 {
                    format!("{}…", &name[..80])
                } else {
                    name.to_string()
                };
                println!("[{}] {}", role, truncated);
            }
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
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "path": path.display().to_string(),
                "updated": false,
            }))?);
        } else {
            println!("Skill already up to date: {}", path.display());
        }
    } else {
        std::fs::write(&path, BUNDLED_SKILL)?;
        if cli.json {
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "path": path.display().to_string(),
                "updated": true,
            }))?);
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
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "path": path.display().to_string(),
            "up_to_date": up_to_date,
        }))?);
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
        Command::Snapshot { depth, tab } => cmd_snapshot(&cli, *depth, tab).await,
        Command::Skill { action } => match action {
            SkillAction::Install => cmd_skill_install(&cli),
            SkillAction::Check => cmd_skill_check(&cli),
        },
        Command::Setup => cmd_setup(),
    }
}
