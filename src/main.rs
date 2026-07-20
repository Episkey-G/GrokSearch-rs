use std::io::{IsTerminal, Write};

use grok_search_rs::config::{self, AuthMode, Config, InitOutcome};

fn main() -> anyhow::Result<()> {
    build_runtime()?.block_on(async_main())
}

/// Multi-threaded runtime for the concurrent HTTP server build.
#[cfg(feature = "http")]
fn build_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
}

/// Single-threaded runtime for the default stdio build — mirrors the previous
/// `#[tokio::main(flavor = "current_thread")]` so the stdio path is unchanged.
#[cfg(not(feature = "http"))]
fn build_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}

async fn async_main() -> anyhow::Result<()> {
    // CLI shim: handle --version, --init before MCP server mode.
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args
        .iter()
        .any(|a| a == "--version" || a == "-V" || a == "-v")
    {
        println!("grok-search-rs {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args.iter().any(|a| a == "init" || a == "--init") {
        return run_init();
    }

    if args.first().map(String::as_str) == Some("login") {
        let cfg = Config::load();
        return run_login(&cfg).await;
    }

    if args.first().map(String::as_str) == Some("status") {
        let cfg = Config::load();
        return run_status(&cfg);
    }

    if args.first().map(String::as_str) == Some("logout") {
        let cfg = Config::load();
        return run_logout(&cfg);
    }

    // Native Streamable HTTP transport (feature `http`): opt in with `--http` /
    // `serve`, or GROK_MCP_BIND=host:port. Credentials come only from
    // per-request headers, so this path intentionally ignores server-side keys.
    // stdio stays the default when neither is set — local users are unaffected.
    #[cfg(feature = "http")]
    {
        let wants_http = args.iter().any(|a| a == "--http" || a == "serve");
        let bind_env = std::env::var("GROK_MCP_BIND").ok();
        if wants_http || bind_env.is_some() {
            let addr = bind_env.unwrap_or_else(|| "127.0.0.1:8080".to_string());
            let bind: std::net::SocketAddr = addr
                .parse()
                .map_err(|err| anyhow::anyhow!("invalid GROK_MCP_BIND '{addr}': {err}"))?;
            let base_env: std::collections::HashMap<String, String> =
                std::env::vars().collect();
            return grok_search_rs::http::run_http(base_env, bind).await;
        }
    }

    let cfg = Config::load();

    // Detect interactive run with missing credentials and print a friendly
    // onboarding guide instead of a cryptic error. MCP clients always pipe
    // stdio, so a TTY here means the user ran the binary directly.
    if cfg.grok_auth_mode == AuthMode::ApiKey
        && cfg.grok_api_key.is_none()
        && std::io::stdin().is_terminal()
    {
        print_setup_guide();
        return Ok(());
    }

    let service = grok_search_rs::service::SearchService::new(cfg)?;
    grok_search_rs::mcp::run_stdio(service).await?;
    Ok(())
}

async fn run_login(cfg: &Config) -> anyhow::Result<()> {
    let path = resolve_auth_path(cfg)?;
    let store = grok_search_rs::oauth::login::login(&path, true).await?;
    println!("Login successful.");
    println!("Auth file: {}", path.display());
    if let Some(exp) = grok_search_rs::oauth::token_store::jwt_exp(&store.access_token) {
        println!("Access token expires at unix time: {exp}");
    }
    Ok(())
}

fn run_status(cfg: &Config) -> anyhow::Result<()> {
    let path = resolve_auth_path(cfg)?;
    let status = grok_search_rs::oauth::token_store::auth_status(&path);
    println!("grok-search-rs OAuth status");
    println!("  Auth file: {}", status.path.display());
    println!(
        "  Authenticated: {}",
        if status.authenticated { "yes" } else { "no" }
    );
    println!(
        "  Refresh token: {}",
        if status.refresh_token_present {
            "present"
        } else {
            "missing"
        }
    );
    println!(
        "  Access expires at: {}",
        status
            .access_expires_at
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "  Base URL: {}",
        status.base_url.unwrap_or_else(|| "unknown".to_string())
    );
    Ok(())
}

fn run_logout(cfg: &Config) -> anyhow::Result<()> {
    let path = resolve_auth_path(cfg)?;
    let removed = grok_search_rs::oauth::token_store::delete_token_store(&path)?;
    if removed {
        println!("Removed OAuth token file: {}", path.display());
    } else {
        println!("No OAuth token file found: {}", path.display());
    }
    Ok(())
}

fn resolve_auth_path(cfg: &Config) -> anyhow::Result<std::path::PathBuf> {
    cfg.grok_auth_file
        .clone()
        .or_else(config::auth_path)
        .ok_or_else(|| anyhow::anyhow!("cannot resolve OAuth auth path; set GROK_SEARCH_AUTH_FILE"))
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

OAuth alternative
  grok-search-rs login
  Set GROK_SEARCH_AUTH_MODE=oauth in your MCP env or config.
  OAuth mode reuses Hermes' xAI client_id and may carry account / terms risk.

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
