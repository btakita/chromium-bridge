use anyhow::{Context, Result, bail};
use base64::Engine;
use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::Message;

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

#[derive(Serialize)]
struct CdpCommand {
    id: u64,
    method: String,
    params: serde_json::Value,
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

async fn ws_command(
    ws_url: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value> {
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url)
        .await
        .context("Failed to connect WebSocket to browser")?;

    let cmd = CdpCommand {
        id: 1,
        method: method.to_string(),
        params,
    };
    ws.send(Message::Text(serde_json::to_string(&cmd)?.into()))
        .await?;

    while let Some(msg) = ws.next().await {
        let msg = msg?;
        if let Message::Text(text) = msg {
            let resp: serde_json::Value = serde_json::from_str(&text)?;
            if resp.get("id") == Some(&serde_json::json!(1)) {
                if let Some(error) = resp.get("error") {
                    bail!("CDP error: {}", error);
                }
                return Ok(resp["result"].clone());
            }
        }
    }
    bail!("WebSocket closed without response")
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

async fn cmd_navigate(cli: &Cli, url: &str) -> Result<()> {
    let tabs = get_tabs(cli).await?;
    let pages: Vec<&Tab> = tabs.iter().filter(|t| t.tab_type == "page").collect();
    let tab = pages.first().context("No open tabs")?;

    let result = ws_command(
        &tab.web_socket_debugger_url,
        "Page.navigate",
        serde_json::json!({ "url": url }),
    )
    .await?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("Navigated to {}", url);
    }
    Ok(())
}

async fn cmd_evaluate(cli: &Cli, expression: &str, tab_index: usize) -> Result<()> {
    let tabs = get_tabs(cli).await?;
    let pages: Vec<&Tab> = tabs.iter().filter(|t| t.tab_type == "page").collect();
    let tab = pages
        .get(tab_index)
        .context(format!("No tab at index {}", tab_index))?;

    let result = ws_command(
        &tab.web_socket_debugger_url,
        "Runtime.evaluate",
        serde_json::json!({
            "expression": expression,
            "returnByValue": true,
        }),
    )
    .await?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else if let Some(value) = result.get("result").and_then(|r| r.get("value")) {
        match value {
            serde_json::Value::String(s) => println!("{}", s),
            other => println!("{}", other),
        }
    } else if let Some(desc) = result
        .get("result")
        .and_then(|r| r.get("description"))
        .and_then(|d| d.as_str())
    {
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
    let tabs = get_tabs(cli).await?;
    let pages: Vec<&Tab> = tabs.iter().filter(|t| t.tab_type == "page").collect();
    let tab = pages
        .get(tab_index)
        .context(format!("No tab at index {}", tab_index))?;
    let ws_url = &tab.web_socket_debugger_url;

    if let Some(url) = url {
        ws_command(ws_url, "Page.navigate", serde_json::json!({ "url": url })).await?;
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
    }

    let result = ws_command(
        ws_url,
        "Page.captureScreenshot",
        serde_json::json!({ "format": "png" }),
    )
    .await?;

    let data = result
        .get("data")
        .and_then(|d| d.as_str())
        .context("No screenshot data in response")?;

    if let Some(path) = output {
        let bytes = base64::engine::general_purpose::STANDARD.decode(data)?;
        std::fs::write(path, bytes)?;
        eprintln!("Screenshot saved to {}", path);
    } else {
        println!("{}", data);
    }
    Ok(())
}

async fn cmd_markdown(cli: &Cli, url: &str, tab_index: usize) -> Result<()> {
    let tabs = get_tabs(cli).await?;
    let pages: Vec<&Tab> = tabs.iter().filter(|t| t.tab_type == "page").collect();
    let tab = pages
        .get(tab_index)
        .context(format!("No tab at index {}", tab_index))?;
    let ws_url = &tab.web_socket_debugger_url;

    ws_command(ws_url, "Page.navigate", serde_json::json!({ "url": url })).await?;
    tokio::time::sleep(std::time::Duration::from_millis(3000)).await;

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

    let result = ws_command(
        ws_url,
        "Runtime.evaluate",
        serde_json::json!({
            "expression": js,
            "returnByValue": true,
        }),
    )
    .await?;

    if let Some(value) = result
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_str())
    {
        println!("{}", value);
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
        Command::Navigate { url } => cmd_navigate(&cli, url).await,
        Command::Evaluate { expression, tab } => cmd_evaluate(&cli, expression, *tab).await,
        Command::Screenshot { url, output, tab } => {
            cmd_screenshot(&cli, url.as_deref(), output.as_deref(), *tab).await
        }
        Command::Markdown { url, tab } => cmd_markdown(&cli, url, *tab).await,
        Command::Setup => cmd_setup(),
    }
}
