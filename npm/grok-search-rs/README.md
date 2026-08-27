# grok-search-rs

Install the native GrokSearch-rs MCP server and run its guided setup:

```bash
npm install -g grok-search-rs
grok-search-rs setup
```

The wizard creates one shared config, then prints a key-free Claude Code or Codex
registration command. Diagnose configuration and configured providers with:

```bash
grok-search-rs doctor
grok-search-rs doctor --json
```

See https://github.com/Episkey-G/GrokSearch-rs for transport details, manual
configuration, and MCP tool examples.
