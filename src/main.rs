use std::io::{IsTerminal, Write};

use grok_search_rs::config::{self, Config, InitOutcome};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // CLI shim: handle --version, --init before MCP server mode.
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--version" || a == "-V" || a == "-v") {
        println!("grok-search-rs {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args.iter().any(|a| a == "init" || a == "--init") {
        return run_init();
    }

    let cfg = Config::load();

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

/// Scaffold the global config file. Idempotent: existing files are reported
/// and left untouched. Prints the resolved path so the user can `$EDITOR` it.
fn run_init() -> anyhow::Result<()> {
    let path = config::config_path().ok_or_else(|| {
        anyhow::anyhow!(
            "cannot resolve config path: set GROK_SEARCH_CONFIG to an explicit file path, \
             or ensure HOME (Unix / Git Bash) or USERPROFILE (Windows) is set"
        )
    })?;
    match config::write_template(&path)? {
        InitOutcome::Created => {
            println!("✓ wrote template: {}", path.display());
            println!("  edit it and uncomment the keys you need.");
        }
        InitOutcome::AlreadyExists => {
            println!("• config already exists: {}", path.display());
            println!("  not overwriting. delete the file first if you want a fresh template.");
        }
    }
    Ok(())
}

fn print_setup_guide() {
    let mut guide = String::from(
        r#"grok-search-rs is an MCP server. It speaks JSON-RPC over stdio and
should be launched by an MCP client (Claude Code, Codex CLI, Gemini CLI,
Cursor, VS Code, Windsurf, ...), not run directly.

Required keys
  GROK_SEARCH_API_KEY   xAI / Grok-compatible key   (https://x.ai/api)
  TAVILY_API_KEY        Tavily fetch + map          (https://tavily.com)
  FIRECRAWL_API_KEY     optional fetch fallback     (https://firecrawl.dev)

One-line install (Claude Code)
  claude mcp add-json grok-search-rs --scope user '{
    "type": "stdio",
    "command": "grok-search-rs",
    "env": {
      "GROK_SEARCH_API_KEY": "xai-...",
      "TAVILY_API_KEY": "tvly-..."
    }
  }'

"#,
    );

    // Hint the global config path only when the file is genuinely missing —
    // avoids nagging users who have already set one up.
    if let Some(path) = config::config_path() {
        if !path.exists() {
            guide.push_str(&format!(
                r#"Tip: set keys once for every MCP client
  grok-search-rs --init                  # scaffold {}
  $EDITOR {}    # uncomment and fill

"#,
                path.display(),
                path.display()
            ));
        }
    }

    guide.push_str(
        r#"Docs:    https://github.com/Episkey-G/GrokSearch-rs#readme
Issues:  https://github.com/Episkey-G/GrokSearch-rs/issues
"#,
    );

    let stdout = std::io::stdout();
    let _ = stdout.lock().write_all(guide.as_bytes());
}
