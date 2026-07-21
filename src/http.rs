//! Native Streamable HTTP transport (Cargo feature `http`).
//!
//! Exposes the same MCP tools as the stdio transport over a single
//! `POST /mcp` endpoint, but multi-tenant: the server process holds **no**
//! credentials. Each request carries the caller's own API keys in headers
//! (`X-Grok-Api-Key` / `X-Tavily-Api-Key` / `X-Firecrawl-Api-Key` /
//! `X-GitHub-Token`); a fully-credentialed [`SearchService`] is built per
//! request via [`SearchService::for_request`], reusing one shared HTTP client
//! and one process-wide source cache (so `get_sources` continuation still
//! works across requests). The whole module is gated behind the `http` feature
//! so the default stdio build never links axum.
//!
//! TLS terminates upstream (Caddy); this server binds loopback only.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::Value;
use tokio::sync::{Mutex, Semaphore};

use crate::cache::SourceCache;
use crate::config::Config;
use crate::error::GrokSearchError;
use crate::mcp::{error_response, handle_message};
use crate::service::SearchService;

/// Max JSON request body. MCP tool calls are tiny; anything larger is abuse.
const MAX_BODY_BYTES: usize = 64 * 1024;

/// Max concurrent in-flight requests; excess gets 429 to protect the 1 GB box.
const MAX_CONCURRENT_REQUESTS: usize = 32;

/// Max concurrent host resolutions (caller-gateway + fetch-tool SSRF
/// validation); excess gets 503. Bounds how many (potentially hung) blocking
/// getaddrinfo calls can run at once.
const MAX_DNS_LOOKUPS: usize = 8;

/// Protocol revisions the Streamable HTTP transport implements. Excludes
/// 2024-11-05 (the deprecated HTTP+SSE transport this endpoint does not serve)
/// and 2025-03-26 (which still mandates JSON-RPC batching — removed only in
/// 2025-06-18; since this endpoint rejects batches, it declares 2025-06-18+ only).
const HTTP_PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18"];

/// Operator-set env keys that must NEVER survive into a per-request config:
/// credentials come only from request headers, never from the server process
/// environment. Stripped from the operator base env once at startup so a stray
/// server-side key can never leak into a tenant's request.
const SECRET_ENV_KEYS: &[&str] = &[
    "GROK_SEARCH_API_KEY",
    "TAVILY_API_KEY",
    "FIRECRAWL_API_KEY",
    "GITHUB_TOKEN",
    "OPENAI_COMPATIBLE_API_KEY",
    "OPENAI_COMPATIBLE_API_URL",
    "OPENAI_COMPATIBLE_MODEL",
    "GROK_SEARCH_AUTH_MODE",
    "GROK_SEARCH_AUTH_FILE",
];

/// Request header -> config env key overlay. Header names match on a
/// case-insensitive basis (axum lowercases header names).
const HEADER_TO_ENV: &[(&str, &str)] = &[
    ("x-grok-api-key", "GROK_SEARCH_API_KEY"),
    ("x-tavily-api-key", "TAVILY_API_KEY"),
    ("x-firecrawl-api-key", "FIRECRAWL_API_KEY"),
    ("x-github-token", "GITHUB_TOKEN"),
];

#[derive(Clone)]
struct AppState {
    /// Shared across every request: one connection pool, operator timeout.
    http_client: reqwest::Client,
    /// One process-wide cache so `get_sources` continuation survives requests.
    cache: Arc<Mutex<SourceCache>>,
    /// Operator non-secret defaults (timeout, budgets, …), secrets stripped.
    base_env: Arc<HashMap<String, String>>,
    /// Allowed `Origin` values; `None` means no allowlist configured (allow).
    allowed_origins: Arc<Option<HashSet<String>>>,
    /// Bounds concurrent in-flight requests (DoS protection on a small box).
    limiter: Arc<Semaphore>,
    /// Caps concurrent host resolutions (gateway + fetch-tool validation) so
    /// hung blocking getaddrinfo calls (which outlive their timeout) can't
    /// pile up threads.
    dns_limiter: Arc<Semaphore>,
    /// Operator request timeout, reused to build a per-request DNS-pinned client
    /// for a caller-supplied gateway (`X-Grok-Base-Url`).
    timeout: std::time::Duration,
}

/// Run the HTTP transport, binding `bind` (loopback in production, behind
/// Caddy). `base_env` is the operator process environment; its non-secret
/// entries seed every request's config, secrets are stripped.
pub async fn run_http(base_env: HashMap<String, String>, bind: SocketAddr) -> anyhow::Result<()> {
    // Operator (non-secret) config drives the shared client + cache sizing.
    let operator_cfg = Config::from_env_map(base_env.clone());
    // Restricted client: rejects redirects to non-public IP-literal targets.
    let http_client = crate::providers::http::build_restricted_client(operator_cfg.timeout);
    let cache = Arc::new(Mutex::new(SourceCache::new(operator_cfg.cache_size)));
    let allowed_origins = parse_allowed_origins(&base_env);

    let state = AppState {
        http_client,
        cache,
        base_env: Arc::new(strip_secrets(base_env)),
        allowed_origins: Arc::new(allowed_origins),
        limiter: Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS)),
        dns_limiter: Arc::new(Semaphore::new(MAX_DNS_LOOKUPS)),
        timeout: operator_cfg.timeout,
    };

    // Only POST is registered; axum answers other methods on /mcp with 405.
    // Body size is capped inside the handler (after the concurrency permit is
    // held), so no DefaultBodyLimit layer is needed.
    let app = Router::new()
        .route("/mcp", post(mcp_post))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    eprintln!("grok-search-rs: Streamable HTTP transport listening on http://{bind}/mcp");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn mcp_post(State(state): State<AppState>, request: axum::extract::Request) -> Response {
    // 0. Concurrency cap FIRST — acquire the permit BEFORE buffering the body,
    //    so slow or oversized request bodies can't tie up memory/connections
    //    past the cap without ever hitting 429.
    let _permit = match state.limiter.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return (StatusCode::TOO_MANY_REQUESTS, "server at capacity").into_response(),
    };

    let (parts, body) = request.into_parts();
    let headers = parts.headers;

    // Read the body under a hard size cap, now that the permit is held.
    let body = match axum::body::to_bytes(body, MAX_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return (StatusCode::PAYLOAD_TOO_LARGE, "request body too large").into_response()
        }
    };

    // 1. Origin validation (DNS-rebinding defense). Absent Origin (non-browser
    //    clients) is allowed; a present Origin must be on the allowlist when one
    //    is configured. Enforced server-side, not via CORS alone.
    if let Some(origin) = header_str(&headers, "origin") {
        if let Some(allowed) = state.allowed_origins.as_ref() {
            if !allowed.contains(origin) {
                return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
            }
        }
    }

    // 2. Parse the JSON-RPC body ourselves so we control the error shape.
    let mut request: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => {
            return json_rpc_error(
                StatusCode::BAD_REQUEST,
                Value::Null,
                -32700,
                format!("parse error: {err}"),
            );
        }
    };

    // 3. Batching was removed in protocol 2025-06-18+; reject arrays.
    if request.is_array() {
        return json_rpc_error(
            StatusCode::BAD_REQUEST,
            Value::Null,
            -32600,
            "batch requests are not supported".to_string(),
        );
    }

    let id = request.get("id").cloned().unwrap_or(Value::Null);

    // 3b. Streamable HTTP protocol-version handling. This endpoint implements
    //     only Streamable HTTP (not the deprecated 2024-11-05 HTTP+SSE
    //     transport): a non-initialize request with an unsupported
    //     MCP-Protocol-Version header -> 400; an initialize that asks for a
    //     non-Streamable-HTTP version is never echoed back (we declare our
    //     latest instead), so clients can't negotiate a transport we don't serve.
    if request.get("method").and_then(Value::as_str) == Some("initialize") {
        let requested = request
            .pointer("/params/protocolVersion")
            .and_then(Value::as_str);
        if requested.is_some_and(|version| !HTTP_PROTOCOL_VERSIONS.contains(&version)) {
            if let Some(params) = request.get_mut("params").and_then(Value::as_object_mut) {
                params.insert(
                    "protocolVersion".to_string(),
                    Value::from(crate::mcp::LATEST_PROTOCOL_VERSION),
                );
            }
        }
    } else if let Some(version) = header_str(&headers, "mcp-protocol-version") {
        if !HTTP_PROTOCOL_VERSIONS.contains(&version) {
            return json_rpc_error(
                StatusCode::BAD_REQUEST,
                id,
                -32600,
                format!("unsupported MCP-Protocol-Version: {version}"),
            );
        }
    }

    // 4. Derive a per-request config from operator defaults + header keys, then
    //    build a request-scoped, fully-credentialed service. Missing required
    //    key -> 401 (fail-closed); OAuth -> 400. Never falls back to a server key.
    let gateway = resolve_gateway(&headers);
    let config = request_config(&state.base_env, &headers, gateway.as_deref());

    // Build with the shared client FIRST so a missing/invalid key fails fast
    // (401) BEFORE any gateway DNS: an unauthenticated request must never reach
    // the resolver, else a bogus/slow `X-Grok-Base-Url` host would hold a
    // concurrency permit until resolution (unauthenticated resolver DoS).
    let service = match SearchService::for_request(
        state.http_client.clone(),
        state.cache.clone(),
        config.clone(),
    ) {
        Ok(service) => service,
        Err(err) => return for_request_error(id.clone(), err),
    };

    // Multi-gateway: a client may point at any Grok-compatible gateway via
    // X-Grok-Base-Url (BYO gateway + matching key, same freedom as stdio). Its
    // host is SSRF-validated AND the outbound connection is PINNED to the
    // validated public IPs, so reqwest cannot re-resolve the hostname to an
    // internal / metadata address between the check and the request
    // (DNS-rebinding SSRF).
    let service = if let Some(url) = gateway.as_deref() {
        // Caller gateways must be HTTPS: the request carries the caller's bearer
        // key, so a plaintext http:// gateway (typo/downgrade) would leak it on
        // the wire. (URL-fetch tools still allow http; this is the gateway only.)
        if !gateway_is_https(url) {
            return json_rpc_error(
                StatusCode::BAD_REQUEST,
                id.clone(),
                -32602,
                "X-Grok-Base-Url must use https".to_string(),
            );
        }
        // Cap concurrent gateway-host resolutions: a blocking getaddrinfo can
        // outlive its 5s timeout, so bound how many run at once (the permit is
        // held until the lookup returns) — a hung host can't pile up threads.
        let dns_permit = match state.dns_limiter.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                return json_rpc_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    id.clone(),
                    -32603,
                    "gateway resolver busy".to_string(),
                )
            }
        };
        let addrs = match validate_public_url(url, Some(dns_permit)).await {
            Ok(addrs) => addrs,
            Err((status, message)) => return json_rpc_error(status, id.clone(), -32602, message),
        };
        let client = match pinned_gateway_client(url, &addrs, state.timeout) {
            Ok(client) => client,
            Err((status, message)) => return json_rpc_error(status, id.clone(), -32602, message),
        };
        // The pinned, no-redirect client applies to the GROK provider only;
        // Tavily/Firecrawl/source fetching keep the shared restricted client
        // (which follows redirects, re-validating every hop).
        match SearchService::for_request_with_grok_client(
            state.http_client.clone(),
            client,
            state.cache.clone(),
            config,
        ) {
            Ok(service) => service,
            Err(err) => return for_request_error(id.clone(), err),
        }
    } else {
        service
    };

    // 5. Clamp abusable numeric args (DoS) — HTTP path only; stdio is untouched.
    clamp_request_args(&mut request);

    // 6. SSRF guard: for URL-fetching tools, block non-public / bad-scheme
    //    targets before any server-side request is made. HTTP path only, so
    //    local stdio users keep full fetch capability (e.g. localhost). The
    //    hostname lookup takes a DNS permit too: a timed-out getaddrinfo keeps
    //    its blocking thread alive, so the same semaphore that bounds gateway
    //    lookups must bound fetch-validation lookups.
    if let Some(url) = fetch_tool_url(&request) {
        let dns_permit = match state.dns_limiter.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                return json_rpc_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    id,
                    -32603,
                    "resolver busy".to_string(),
                )
            }
        };
        if let Err((status, message)) = validate_public_url(url, Some(dns_permit)).await {
            return json_rpc_error(status, id, -32602, message);
        }
    }

    // 7. Dispatch through the shared, transport-agnostic handler. Respond as an
    //    SSE stream when the client accepts text/event-stream (Streamable HTTP
    //    streaming mode), else a single application/json body. `None` = a
    //    notification/response with no reply -> 202 with an empty body.
    match handle_message(&service, request).await {
        Some(response) if wants_sse(&headers) => sse_response(&response),
        Some(response) => (StatusCode::OK, Json(response)).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

/// Whether the client accepts an SSE stream (Streamable HTTP streaming mode).
/// Streamable HTTP clients send `Accept: application/json, text/event-stream`.
fn wants_sse(headers: &HeaderMap) -> bool {
    header_str(headers, "accept")
        .map(|accept| accept.contains("text/event-stream"))
        .unwrap_or(false)
}

/// Frame a single JSON-RPC response as a one-event SSE stream that then closes,
/// per the Streamable HTTP transport: one `message` event carrying the JSON-RPC
/// response, after which the stream ends (these tools are request/response).
fn sse_response(response: &Value) -> Response {
    use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
    let data = serde_json::to_string(response).unwrap_or_else(|_| "{}".to_string());
    let body = format!("event: message\ndata: {data}\n\n");
    let mut resp = Response::new(axum::body::Body::from(body));
    resp.headers_mut().insert(
        CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/event-stream"),
    );
    resp.headers_mut()
        .insert(CACHE_CONTROL, axum::http::HeaderValue::from_static("no-cache"));
    resp
}

/// If `request` is a tools/call for a URL-fetching tool (`web_fetch`/`web_map`),
/// return its `url` argument for SSRF validation.
fn fetch_tool_url(request: &Value) -> Option<&str> {
    if request.get("method").and_then(Value::as_str) != Some("tools/call") {
        return None;
    }
    let params = request.get("params")?;
    match params.get("name").and_then(Value::as_str)? {
        "web_fetch" | "web_map" => params.get("arguments")?.get("url")?.as_str(),
        _ => None,
    }
}

/// Reject a URL that could drive a server-side request at a non-public target
/// (SSRF): bad scheme, or a host that is / resolves to a private, loopback,
/// link-local (incl. cloud metadata), or CGNAT address. On success returns the
/// validated public IP(s) the host resolves to, so the caller can pin the
/// actual connection to them (closing DNS-rebinding between check and use).
async fn validate_public_url(
    raw: &str,
    dns_permit: Option<tokio::sync::OwnedSemaphorePermit>,
) -> Result<Vec<std::net::IpAddr>, (StatusCode, String)> {
    let parsed = url::Url::parse(raw)
        .map_err(|err| (StatusCode::BAD_REQUEST, format!("invalid url: {err}")))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("unsupported url scheme: {other}"),
            ))
        }
    }
    let host = parsed
        .host_str()
        .ok_or((StatusCode::BAD_REQUEST, "url has no host".to_string()))?;

    // IP literal (strip IPv6 brackets): check directly, no DNS lookup.
    let literal = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = literal.parse::<std::net::IpAddr>() {
        return if crate::providers::http::is_public_ip(&ip) {
            Ok(vec![ip])
        } else {
            Err((
                StatusCode::FORBIDDEN,
                "url targets a non-public address".to_string(),
            ))
        };
    }

    // Hostname: resolve (under a hard deadline so a slow resolver can't hold a
    // concurrency permit) and require every resolved address to be public.
    let port = parsed.port_or_known_default().unwrap_or(443);
    let host_owned = host.to_string();
    let resolved = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(move || {
            // Hold the resolver permit until getaddrinfo actually returns: the
            // 5s timeout abandons this task but cannot cancel it, so the permit
            // (not the timeout) is what bounds concurrent hung lookups.
            let _dns_permit = dns_permit;
            use std::net::ToSocketAddrs;
            (host_owned.as_str(), port)
                .to_socket_addrs()
                .map(|iter| iter.map(|addr| addr.ip()).collect::<Vec<_>>())
        }),
    )
    .await
    .map_err(|_| {
        (
            StatusCode::GATEWAY_TIMEOUT,
            "host resolution timed out".to_string(),
        )
    })?
    .map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("resolver task failed: {err}"),
        )
    })?
    .map_err(|err| (StatusCode::BAD_REQUEST, format!("cannot resolve host: {err}")))?;

    if resolved.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "host did not resolve".to_string()));
    }
    for ip in &resolved {
        if !crate::providers::http::is_public_ip(ip) {
            return Err((
                StatusCode::FORBIDDEN,
                "url resolves to a non-public address".to_string(),
            ));
        }
    }
    Ok(resolved)
}

/// Build a restricted client whose DNS for the gateway `host` is pinned to
/// `addrs` — the IPs [`validate_public_url`] just verified are public, at the
/// gateway port. Connecting only to a pre-validated address closes the
/// DNS-rebinding window between the SSRF check and the actual request.
fn pinned_gateway_client(
    url: &str,
    addrs: &[std::net::IpAddr],
    timeout: std::time::Duration,
) -> Result<reqwest::Client, (StatusCode, String)> {
    let parsed = url::Url::parse(url)
        .map_err(|err| (StatusCode::BAD_REQUEST, format!("invalid url: {err}")))?;
    let host = parsed
        .host_str()
        .ok_or((StatusCode::BAD_REQUEST, "url has no host".to_string()))?;
    let port = parsed.port_or_known_default().unwrap_or(443);
    let socket_addrs: Vec<std::net::SocketAddr> = addrs
        .iter()
        .map(|ip| std::net::SocketAddr::new(*ip, port))
        .collect();
    Ok(crate::providers::http::build_restricted_client_pinned(
        timeout,
        host,
        &socket_addrs,
    ))
}

/// Map a `SearchService::for_request` construction error to a JSON-RPC HTTP
/// response: a missing key -> 401 (fail-closed), OAuth / other -> 400.
fn for_request_error(id: Value, err: GrokSearchError) -> Response {
    let status = match err {
        GrokSearchError::MissingConfig(_) => StatusCode::UNAUTHORIZED,
        _ => StatusCode::BAD_REQUEST,
    };
    json_rpc_error(status, id, err.code() as i64, err.to_string())
}

/// Clamp abusable numeric tool arguments to sane upper bounds so a single
/// public request cannot ask for unbounded work.
fn clamp_request_args(request: &mut Value) {
    if request.get("method").and_then(Value::as_str) != Some("tools/call") {
        return;
    }
    let Some(params) = request.get_mut("params") else {
        return;
    };
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string);
    let Some(args) = params.get_mut("arguments").and_then(Value::as_object_mut) else {
        return;
    };
    match name.as_deref() {
        Some("web_search") => {
            clamp_u64(args, "extra_sources", 50);
            clamp_u64(args, "recency_days", 3650);
        }
        // Enforce the fetch cap even when max_chars is absent or non-integer, so
        // a public caller can't bypass it by omitting the argument (the operator
        // default fetch cap is unbounded).
        Some("web_fetch") => cap_u64(args, "max_chars", 5_000_000),
        Some("web_map") => clamp_u64(args, "max_results", 100),
        _ => {}
    }
}

fn clamp_u64(args: &mut serde_json::Map<String, Value>, key: &str, max: u64) {
    if let Some(value) = args.get(key).and_then(Value::as_u64) {
        if value > max {
            args.insert(key.to_string(), Value::from(max));
        }
    }
}

/// Like [`clamp_u64`], but also enforces `max` when the argument is absent or
/// non-integer — so a caller cannot bypass the cap by omitting it.
fn cap_u64(args: &mut serde_json::Map<String, Value>, key: &str, max: u64) {
    match args.get(key).and_then(Value::as_u64) {
        Some(value) if value <= max => {}
        _ => {
            args.insert(key.to_string(), Value::from(max));
        }
    }
}

/// Build a per-request [`Config`] from the operator base env (secrets already
/// stripped) overlaid with the caller's header-supplied keys.
fn request_config(
    base_env: &HashMap<String, String>,
    headers: &HeaderMap,
    gateway: Option<&str>,
) -> Config {
    let mut map = base_env.clone();
    for (header_name, env_key) in HEADER_TO_ENV {
        if let Some(value) = header_str(headers, header_name) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                map.insert((*env_key).to_string(), trimmed.to_string());
            }
        }
    }
    if let Some(url) = gateway {
        map.insert("GROK_SEARCH_URL".to_string(), url.to_string());
    }
    Config::from_env_map(map)
}

/// The Grok gateway a request targets via `X-Grok-Base-Url`, honored verbatim
/// (BYO gateway). Absent/empty header -> `None` (the operator default gateway
/// is used). The returned URL's host is SSRF-validated by the caller before
/// use, so any *public* gateway is allowed but internal addresses are not.
fn resolve_gateway(headers: &HeaderMap) -> Option<String> {
    header_str(headers, "x-grok-base-url")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn json_rpc_error(status: StatusCode, id: Value, code: i64, message: String) -> Response {
    (status, Json(error_response(id, code, message))).into_response()
}

fn strip_secrets(mut env: HashMap<String, String>) -> HashMap<String, String> {
    for key in SECRET_ENV_KEYS {
        env.remove(*key);
    }
    env
}

/// Parse `GROK_MCP_ALLOWED_ORIGINS` (comma-separated) into an allowlist.
/// Unset/empty -> `None` (no browser-origin restriction; keys are per-request).
fn parse_allowed_origins(env: &HashMap<String, String>) -> Option<HashSet<String>> {
    let raw = env.get("GROK_MCP_ALLOWED_ORIGINS")?;
    let set: HashSet<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();
    if set.is_empty() {
        None
    } else {
        Some(set)
    }
}

/// A caller-supplied gateway must be HTTPS: the request carries the caller's
/// bearer key, so a plaintext `http://` gateway would leak it on the wire.
/// (URL-fetch tools still allow `http`; this restriction is gateway-only.)
fn gateway_is_https(url: &str) -> bool {
    url::Url::parse(url)
        .map(|parsed| parsed.scheme() == "https")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> HashMap<String, String> {
        HashMap::new()
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                value.parse().unwrap(),
            );
        }
        map
    }

    #[test]
    fn request_config_overlays_header_keys() {
        let cfg = request_config(
            &base(),
            &headers(&[
                ("X-Grok-Api-Key", "xai-caller"),
                ("X-Tavily-Api-Key", "tvly-caller"),
            ]),
            None,
        );
        assert_eq!(cfg.grok_api_key.as_deref(), Some("xai-caller"));
        assert_eq!(cfg.tavily_api_key.as_deref(), Some("tvly-caller"));
    }

    #[test]
    fn request_config_applies_gateway_override() {
        let cfg = request_config(
            &base(),
            &headers(&[("X-Grok-Api-Key", "xai-caller")]),
            Some("https://api.x.ai"),
        );
        assert_eq!(cfg.grok_api_url, "https://api.x.ai/v1");
    }

    #[test]
    fn resolve_gateway_reads_header_verbatim() {
        // No header -> operator default (None).
        assert_eq!(resolve_gateway(&headers(&[])), None);
        // Any gateway is honored verbatim (allowlist removed); the host is
        // SSRF-validated separately in the request path.
        assert_eq!(
            resolve_gateway(&headers(&[("X-Grok-Base-Url", "https://api.x.ai")])).as_deref(),
            Some("https://api.x.ai"),
        );
        // Empty / whitespace-only header -> None.
        assert_eq!(
            resolve_gateway(&headers(&[("X-Grok-Base-Url", "   ")])),
            None
        );
    }

    #[test]
    fn request_config_never_inherits_server_secret() {
        // Even if the operator base env carries a key, it is stripped so it can
        // never leak into a tenant request.
        let mut server_env = HashMap::new();
        server_env.insert("GROK_SEARCH_API_KEY".to_string(), "xai-SERVER".to_string());
        let sanitized = strip_secrets(server_env);
        let cfg = request_config(&sanitized, &headers(&[]), None);
        assert_eq!(
            cfg.grok_api_key, None,
            "server key must not survive into a keyless request"
        );
    }

    #[test]
    fn strip_secrets_removes_every_secret_key() {
        let mut env = HashMap::new();
        for key in SECRET_ENV_KEYS {
            env.insert((*key).to_string(), "secret".to_string());
        }
        env.insert("GROK_SEARCH_TIMEOUT_SECONDS".to_string(), "30".to_string());
        let stripped = strip_secrets(env);
        for key in SECRET_ENV_KEYS {
            assert!(!stripped.contains_key(*key), "{key} not stripped");
        }
        // Non-secret operator knobs survive.
        assert_eq!(
            stripped.get("GROK_SEARCH_TIMEOUT_SECONDS").map(String::as_str),
            Some("30")
        );
    }

    #[test]
    fn parse_allowed_origins_handles_unset_and_list() {
        assert!(parse_allowed_origins(&base()).is_none());
        let mut env = HashMap::new();
        env.insert(
            "GROK_MCP_ALLOWED_ORIGINS".to_string(),
            "https://a.example, https://b.example".to_string(),
        );
        let set = parse_allowed_origins(&env).expect("allowlist");
        assert!(set.contains("https://a.example"));
        assert!(set.contains("https://b.example"));
    }

    #[tokio::test]
    async fn validate_public_url_blocks_ssrf_targets() {
        for bad in [
            "http://169.254.169.254/latest/meta-data/", // cloud metadata
            "http://127.0.0.1/",                         // loopback
            "http://10.0.0.5/",                          // private
            "http://192.168.1.1/",                       // private
            "http://100.64.0.1/",                        // CGNAT
            "https://[::1]/",                            // IPv6 loopback
            "file:///etc/passwd",                        // bad scheme
            "gopher://example.com/",                     // bad scheme
        ] {
            assert!(
                validate_public_url(bad, None).await.is_err(),
                "expected {bad} to be rejected"
            );
        }
    }

    #[tokio::test]
    async fn validate_public_url_allows_public_ip_literal() {
        // Public IP literals pass with no DNS lookup, returning the pinned IP.
        assert_eq!(
            validate_public_url("https://1.1.1.1/", None).await.unwrap(),
            vec!["1.1.1.1".parse::<std::net::IpAddr>().unwrap()]
        );
        assert!(validate_public_url("http://8.8.8.8/", None).await.is_ok());
    }

    #[test]
    fn gateway_is_https_rejects_plaintext() {
        assert!(gateway_is_https("https://api.x.ai/v1"));
        assert!(!gateway_is_https("http://api.x.ai/v1"));
        assert!(!gateway_is_https("ftp://api.x.ai"));
        assert!(!gateway_is_https("not a url"));
    }

    #[test]
    fn clamp_request_args_caps_numeric_inputs() {
        let mut request = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {
                "name": "web_search",
                "arguments": { "query": "q", "extra_sources": 9999, "recency_days": 999999 }
            }
        });
        clamp_request_args(&mut request);
        assert_eq!(request["params"]["arguments"]["extra_sources"], 50);
        assert_eq!(request["params"]["arguments"]["recency_days"], 3650);
    }

    #[test]
    fn clamp_request_args_enforces_web_fetch_cap() {
        // Absent max_chars -> cap injected (cannot bypass by omitting it).
        let mut absent = serde_json::json!({
            "method": "tools/call",
            "params": { "name": "web_fetch", "arguments": { "url": "https://example.com" } }
        });
        clamp_request_args(&mut absent);
        assert_eq!(absent["params"]["arguments"]["max_chars"], 5_000_000);
        // Oversized -> clamped down.
        let mut big = serde_json::json!({
            "method": "tools/call",
            "params": { "name": "web_fetch", "arguments": { "url": "x", "max_chars": 999_999_999_u64 } }
        });
        clamp_request_args(&mut big);
        assert_eq!(big["params"]["arguments"]["max_chars"], 5_000_000);
        // Smaller value -> kept.
        let mut small = serde_json::json!({
            "method": "tools/call",
            "params": { "name": "web_fetch", "arguments": { "url": "x", "max_chars": 1000 } }
        });
        clamp_request_args(&mut small);
        assert_eq!(small["params"]["arguments"]["max_chars"], 1000);
    }

    #[test]
    fn fetch_tool_url_targets_fetch_tools_only() {
        let fetch = serde_json::json!({
            "method": "tools/call",
            "params": { "name": "web_fetch", "arguments": { "url": "https://example.com" } }
        });
        assert_eq!(fetch_tool_url(&fetch), Some("https://example.com"));
        let search = serde_json::json!({
            "method": "tools/call",
            "params": { "name": "web_search", "arguments": { "query": "x" } }
        });
        assert_eq!(fetch_tool_url(&search), None);
    }

    #[test]
    fn wants_sse_reads_accept_header() {
        assert!(wants_sse(&headers(&[(
            "Accept",
            "application/json, text/event-stream"
        )])));
        assert!(!wants_sse(&headers(&[("Accept", "application/json")])));
        assert!(!wants_sse(&headers(&[])));
    }

    #[tokio::test]
    async fn sse_response_frames_a_single_message_event() {
        let resp = sse_response(&serde_json::json!({"jsonrpc":"2.0","id":1,"result":{}}));
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CONTENT_TYPE)
                .unwrap(),
            "text/event-stream"
        );
        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.starts_with("event: message\ndata: "));
        assert!(text.ends_with("\n\n"));
        assert!(text.contains("\"jsonrpc\":\"2.0\""));
    }
}
