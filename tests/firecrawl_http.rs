//! Exercise config normalization and both Firecrawl operations against a local
//! gateway. Uses the same std TCP mock approach as the Tavily rotation tests.

use grok_search_rs::config::Config;
use grok_search_rs::providers::firecrawl::FirecrawlProvider;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;
use std::time::Duration;

struct Request {
    line: String,
    authorization: String,
    body: Value,
}

fn spawn_gateway(responses: Vec<Value>) -> (String, JoinHandle<Vec<Request>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind gateway");
    let base = format!("http://{}", listener.local_addr().expect("gateway addr"));
    let handle = std::thread::spawn(move || {
        responses
            .into_iter()
            .map(|response| {
                let (mut stream, _) = listener.accept().expect("accept request");
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("set read timeout");
                let mut reader = BufReader::new(&mut stream);
                let mut line = String::new();
                reader.read_line(&mut line).expect("read request line");
                let mut authorization = String::new();
                let mut content_length = 0;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).expect("read header");
                    if header == "\r\n" || header.is_empty() {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':') {
                        if name.eq_ignore_ascii_case("authorization") {
                            authorization = value.trim().to_string();
                        } else if name.eq_ignore_ascii_case("content-length") {
                            content_length = value.trim().parse().expect("content length");
                        }
                    }
                }
                let mut body = vec![0; content_length];
                reader.read_exact(&mut body).expect("read request body");
                let request = Request {
                    line: line.trim_end().to_string(),
                    authorization,
                    body: serde_json::from_slice(&body).expect("JSON request"),
                };
                let body = response.to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .expect("write response");
                request
            })
            .collect()
    });
    (base, handle)
}

#[tokio::test]
async fn search_and_scrape_preserve_gateway_prefix_and_api_version() {
    for (suffix, version) in [
        ("/gateway/firecrawl/v2/", "v2"),
        ("/gateway/firecrawl/v1/", "v1"),
        ("/gateway/firecrawl", "v2"),
    ] {
        let results = json!([{ "url": "https://example.com", "title": "Example" }]);
        let data = if version == "v2" {
            json!({ "web": results })
        } else {
            results
        };
        let (base, server) = spawn_gateway(vec![
            json!({ "success": true, "data": data }),
            json!({ "success": true, "data": { "markdown": "# Example" } }),
        ]);
        let url = format!("{base}{suffix}");
        let config = Config::from_env_map([
            ("FIRECRAWL_API_URL", url.as_str()),
            ("FIRECRAWL_API_KEY", "fc-test-key"),
        ]);
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let provider = FirecrawlProvider::with_client(
            client,
            config.firecrawl_api_url,
            config.firecrawl_api_key.expect("configured key"),
        );

        let sources = provider.search("example", 3).await.expect("search");
        assert_eq!(sources.len(), 1, "base: {url}");
        assert_eq!(sources[0].url, "https://example.com");
        let page = provider
            .scrape("https://example.com")
            .await
            .expect("scrape");
        assert_eq!(page.content, "# Example");

        let requests = server.join().expect("gateway completed");
        for (request, operation) in requests.iter().zip(["search", "scrape"]) {
            assert_eq!(
                request.line,
                format!("POST /gateway/firecrawl/{version}/{operation} HTTP/1.1"),
                "base: {url}"
            );
            assert_eq!(request.authorization, "Bearer fc-test-key");
        }
        assert_eq!(requests[0].body, json!({ "query": "example", "limit": 3 }));
        assert_eq!(
            requests[1].body,
            json!({ "url": "https://example.com", "formats": ["markdown"] })
        );
    }
}
