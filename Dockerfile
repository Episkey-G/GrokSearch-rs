# syntax=docker/dockerfile:1
#
# grok-search-rs — public multi-tenant Streamable HTTP MCP server.
# The image holds NO credentials: each request carries the caller's own keys as
# headers (X-Grok-Api-Key / X-Tavily-Api-Key / X-Firecrawl-Api-Key).

# ---- build stage ----------------------------------------------------------
FROM rust:1-bookworm AS builder
WORKDIR /app
COPY . .
# release-http => panic=unwind so a handler panic can't abort the whole process.
RUN cargo build --profile release-http --features http \
    && cp target/release-http/grok-search-rs /grok-search-rs

# ---- runtime stage --------------------------------------------------------
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin grokmcp
COPY --from=builder /grok-search-rs /usr/local/bin/grok-search-rs
# Bind all interfaces inside the container; a reverse proxy terminates TLS.
ENV GROK_MCP_BIND=0.0.0.0:8080
EXPOSE 8080
USER grokmcp
ENTRYPOINT ["grok-search-rs", "--http"]
