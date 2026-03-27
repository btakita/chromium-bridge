//! # Module: main (chromium-bridge CLI)
//!
//! ## Spec
//! - Entry point for the `chromium-bridge` binary; parses CLI with `clap` derive.
//! - `Cli` struct holds global options (`--host`, `--port`, `--timeout`, `--json`)
//!   and a `Command` subcommand enum.
//! - Commands: `Check`, `List`, `Navigate`, `Evaluate`, `Screenshot`, `Markdown`, `Setup`.
//! - CDP communication: HTTP (`/json/*`) for tab listing/version, WebSocket via `cdpkit`
//!   for page-level commands (navigate, evaluate, screenshot).
//! - `connect_to_tab` creates a cdpkit `CDP` client and attaches to a specific tab by index.
//! - Screenshot and markdown commands use `LoadEventFired` event streaming for page load
//!   detection instead of fixed delays.
//! - `cmd_markdown` injects a DOM walker JS that converts page content to clean markdown.
//! - `cmd_setup` detects installed Chromium browsers and checks debugging flag status.
//!
//! ## Agentic Contracts
//! - All commands return `anyhow::Result<()>`; errors propagate to stderr.
//! - `--json` flag produces machine-readable output on all commands; human-readable by default.
//! - Tab index defaults to 0 (first page tab); `--tab` overrides.
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
        /// Target tab index (default: first tab)
        #[arg(long, default_value = "0")]
        tab: usize,
    },
    /// Run JavaScript in the active tab
    Evaluate {
        /// JavaScript expression to evaluate
        expression: String,
        /// Target tab index (default: first tab)
        #[arg(long, default_value = "0")]
        tab: usize,
    },
    /// Capture a page screenshot
    Screenshot {
        /// URL to navigate to before capturing (optional)
        url: Option<String>,
        /// Output file path (default: stdout as base64)
        #[arg(short, long)]
        output: Option<String>,
        /// Target tab index (default: first tab)
        #[arg(long, default_value = "0")]
        tab: usize,
    },
    /// Convert a web page to markdown
    Markdown {
        /// URL to convert
        url: String,
        /// Target tab index (default: first tab)
        #[arg(long, default_value = "0")]
        tab: usize,
    },
    /// Configure browser for remote debugging
    Setup,
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

/// Connect to a specific tab via cdpkit, creating a CDP session.
async fn connect_to_tab(cli: &Cli, tab_index: usize) -> Result<(CDP, String)> {
    let tabs = get_tabs(cli).await?;
    let pages: Vec<&Tab> = tabs.iter().filter(|t| t.tab_type == "page").collect();
    let tab = pages
        .get(tab_index)
        .context(format!("No tab at index {}", tab_index))?;

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

async fn cmd_navigate(cli: &Cli, url: &str, tab_index: usize) -> Result<()> {
    let (cdp, session) = connect_to_tab(cli, tab_index).await?;

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

async fn cmd_evaluate(cli: &Cli, expression: &str, tab_index: usize) -> Result<()> {
    let (cdp, session) = connect_to_tab(cli, tab_index).await?;

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
    tab_index: usize,
) -> Result<()> {
    let (cdp, session) = connect_to_tab(cli, tab_index).await?;

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

async fn cmd_markdown(cli: &Cli, url: &str, tab_index: usize) -> Result<()> {
    let (cdp, session) = connect_to_tab(cli, tab_index).await?;

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
        Command::Navigate { url, tab } => cmd_navigate(&cli, url, *tab).await,
        Command::Evaluate { expression, tab } => cmd_evaluate(&cli, expression, *tab).await,
        Command::Screenshot { url, output, tab } => {
            cmd_screenshot(&cli, url.as_deref(), output.as_deref(), *tab).await
        }
        Command::Markdown { url, tab } => cmd_markdown(&cli, url, *tab).await,
        Command::Setup => cmd_setup(),
    }
}
