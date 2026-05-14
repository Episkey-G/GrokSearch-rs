#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = grok_search_rs::config::Config::from_env();
    let service = grok_search_rs::service::SearchService::new(cfg)?;
    grok_search_rs::mcp::run_stdio(service).await?;
    Ok(())
}
