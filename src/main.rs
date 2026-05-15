use std::io::{IsTerminal, Write};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = grok_search_rs::config::Config::from_env();

    // Detect interactive run with missing credentials and print a friendly
    // onboarding guide instead of a cryptic error. MCP clients always pipe
    // stdio, so a TTY here means the user ran the binary directly.
    if cfg.grok_api_key.is_none() && std::io::stdin().is_terminal() {
        print_setup_guide();
        return Ok(());
    }

    let service = grok_search_rs::service::SearchService::new(cfg)?;
    grok_search_rs::mcp::run_stdio(service).await?;
    Ok(())
}

fn print_setup_guide() {
    let guide = r#"grok-search-rs is an MCP server. It speaks JSON-RPC over stdio and
should be launched by an MCP client (Claude Code, Codex CLI, Gemini CLI,
Cursor, VS Code, Windsurf, ...), not run directly.

Required keys
  GROK_SEARCH_API_KEY   xAI / Grok-compatible key   (https://x.ai/api)
  TAVILY_API_KEY        Tavily fetch + map          (https://tavily.com)
  FIRECRAWL_API_KEY     optional fetch fallback     (https://firecrawl.dev)

One-line install (Claude Code)
  claude mcp add-json grok-search-rs --scope user '{
    "type": "stdio",
    "command": "npx",
    "args": ["-y", "grok-search-rs"],
    "env": {
      "GROK_SEARCH_API_KEY": "xai-...",
      "TAVILY_API_KEY": "tvly-..."
    }
  }'

Docs:    https://github.com/Episkey-G/GrokSearch-rs#readme
Issues:  https://github.com/Episkey-G/GrokSearch-rs/issues
"#;
    let stdout = std::io::stdout();
    let _ = stdout.lock().write_all(guide.as_bytes());
}
