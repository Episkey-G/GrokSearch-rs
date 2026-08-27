//! Interactive, first-run configuration wizard.
//!
//! The public entrypoint is deliberately small: [`run_interactive`] requires a
//! terminal, writes a new private config without overwriting an existing one,
//! and returns whether the caller should run the live doctor. The module itself
//! never performs network I/O or edits third-party MCP client configuration.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;

use anyhow::{bail, Context};

use crate::config::{config_path, write_private_config, Config, InitOutcome};

const MAX_ATTEMPTS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupResult {
    /// The user explicitly opted into live provider checks after setup.
    /// The caller owns that network action; this module never performs it.
    pub run_doctor: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Backend {
    XaiResponses,
    OpenAiCompatible,
}

impl Backend {
    fn label(self) -> &'static str {
        match self {
            Self::XaiResponses => "xAI Responses",
            Self::OpenAiCompatible => "OpenAI-compatible Chat Completions",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClientChoice {
    ClaudeCode,
    Codex,
    Skip,
}

impl ClientChoice {
    fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Skip => "skip",
        }
    }

    fn command(self) -> Option<&'static str> {
        match self {
            Self::ClaudeCode => {
                Some("claude mcp add --scope user grok-search-rs -- grok-search-rs")
            }
            Self::Codex => Some("codex mcp add grok-search-rs -- grok-search-rs"),
            Self::Skip => None,
        }
    }
}

/// Secret-bearing setup state. Intentionally does not implement `Debug`.
struct SetupAnswers {
    backend: Backend,
    api_url: String,
    model: String,
    api_key: String,
    tavily_api_key: Option<String>,
    firecrawl_api_key: Option<String>,
    github_token: Option<String>,
    client: ClientChoice,
}

trait PromptInput {
    fn read_prompt_line(&mut self, buffer: &mut String) -> io::Result<usize>;
}

impl<T: BufRead> PromptInput for T {
    fn read_prompt_line(&mut self, buffer: &mut String) -> io::Result<usize> {
        self.read_line(buffer)
    }
}

/// Reads one terminal line at a time without retaining a `StdinLock`. The
/// secret reader must be able to lock the same terminal between normal prompts.
struct TerminalPromptInput;

impl PromptInput for TerminalPromptInput {
    fn read_prompt_line(&mut self, buffer: &mut String) -> io::Result<usize> {
        io::stdin().read_line(buffer)
    }
}

trait SecretInput {
    fn read_secret(&mut self) -> io::Result<String>;
}

struct TerminalSecretInput;

impl SecretInput for TerminalSecretInput {
    fn read_secret(&mut self) -> io::Result<String> {
        rpassword::read_password()
    }
}

/// Run the first-use setup wizard against the process terminal.
///
/// This function performs local terminal and filesystem I/O only. When the
/// returned `run_doctor` flag is true, the caller may perform the live probe.
pub fn run_interactive() -> anyhow::Result<SetupResult> {
    let stdin = io::stdin();
    if !stdin.is_terminal() {
        bail!(
            "setup requires an interactive terminal; run `grok-search-rs setup` directly in a terminal"
        );
    }

    let path = config_path().ok_or_else(|| {
        anyhow::anyhow!(
            "cannot resolve config path: set GROK_SEARCH_CONFIG or ensure HOME/USERPROFILE is set"
        )
    })?;

    let mut input = TerminalPromptInput;
    let mut secrets = TerminalSecretInput;
    let stdout = io::stdout();
    let mut output = stdout.lock();
    run_with_io(true, &path, &mut input, &mut secrets, &mut output)
}

fn run_with_io<I, S, W>(
    is_tty: bool,
    path: &Path,
    input: &mut I,
    secrets: &mut S,
    output: &mut W,
) -> anyhow::Result<SetupResult>
where
    I: PromptInput,
    S: SecretInput,
    W: Write,
{
    if !is_tty {
        bail!(
            "setup requires an interactive terminal; run `grok-search-rs setup` directly in a terminal"
        );
    }
    if path.exists() {
        bail!(
            "configuration already exists at {}; no changes were made. Run `grok-search-rs doctor` to verify it, edit it manually, or set GROK_SEARCH_CONFIG to a new path",
            path.display()
        );
    }

    let defaults = Config::from_env_map(Vec::<(String, String)>::new());
    writeln!(output, "GrokSearch-rs setup")?;
    writeln!(output)?;
    writeln!(output, "Config file: {}", path.display())?;
    writeln!(
        output,
        "This wizard stores API keys in that configuration file."
    )?;
    #[cfg(windows)]
    writeln!(
        output,
        "Windows inherits the parent directory ACL; use the default profile path or another private directory."
    )?;
    writeln!(
        output,
        "It will not contact any provider unless you opt in after saving."
    )?;
    writeln!(output)?;

    let backend = prompt_backend(input, output)?;
    let (api_url, model, api_key) = match backend {
        Backend::XaiResponses => {
            let api_url = prompt_url(
                input,
                output,
                "xAI API base URL",
                Some(defaults.grok_api_url.as_str()),
            )?;
            let model = prompt_model(
                input,
                output,
                "xAI model",
                Some(defaults.grok_model.as_str()),
            )?;
            let key = prompt_required_secret(output, secrets, "xAI API key")?;
            (api_url, model, key)
        }
        Backend::OpenAiCompatible => {
            let api_url = prompt_url(input, output, "OpenAI-compatible gateway URL", None)?;
            let model = prompt_model(input, output, "Gateway model", None)?;
            let key = prompt_required_secret(output, secrets, "Gateway API key")?;
            (api_url, model, key)
        }
    };

    writeln!(output)?;
    writeln!(
        output,
        "Optional source providers (press Enter to skip any key):"
    )?;
    let tavily_api_key = prompt_optional_secret(output, secrets, "Tavily API key")?;
    let firecrawl_api_key = prompt_optional_secret(output, secrets, "Firecrawl API key")?;
    let github_token = prompt_optional_secret(output, secrets, "GitHub token")?;
    let client = prompt_client(input, output)?;

    let answers = SetupAnswers {
        backend,
        api_url,
        model,
        api_key,
        tavily_api_key,
        firecrawl_api_key,
        github_token,
        client,
    };

    write_redacted_summary(output, path, &answers)?;
    if !prompt_yes_no(input, output, "Write this configuration?", true)? {
        writeln!(output, "Setup cancelled. No changes were made.")?;
        return Ok(SetupResult { run_doctor: false });
    }

    let body = render_config(&answers)?;
    match write_private_config(path, &body)
        .with_context(|| format!("write private config {}", path.display()))?
    {
        InitOutcome::Created => {}
        InitOutcome::AlreadyExists => {
            bail!(
                "configuration appeared at {} while setup was running; no file was overwritten",
                path.display()
            )
        }
    }

    writeln!(output)?;
    writeln!(output, "Configuration saved: {}", path.display())?;
    writeln!(output, "Offline TOML validation passed.")?;
    if let Some(command) = answers.client.command() {
        writeln!(output)?;
        writeln!(output, "Add the MCP server to {}:", answers.client.label())?;
        writeln!(output, "  {command}")?;
    }
    writeln!(output)?;
    writeln!(output, "First search prompt:")?;
    writeln!(
        output,
        "  Use grok-search-rs web_search to find the latest Rust MCP SDK release and cite the sources."
    )?;
    writeln!(output)?;
    let run_doctor = prompt_yes_no(
        input,
        output,
        "Run live provider checks now? This sends requests to configured upstreams.",
        false,
    )?;

    Ok(SetupResult { run_doctor })
}

fn prompt_backend<I: PromptInput, W: Write>(
    input: &mut I,
    output: &mut W,
) -> anyhow::Result<Backend> {
    writeln!(output, "Choose the AI backend:")?;
    writeln!(output, "  1. xAI Responses (recommended)")?;
    writeln!(output, "  2. OpenAI-compatible Chat Completions")?;
    for _ in 0..MAX_ATTEMPTS {
        let value = prompt_line(input, output, "Selection [1]: ")?;
        match value.to_ascii_lowercase().as_str() {
            "" | "1" | "xai" => return Ok(Backend::XaiResponses),
            "2" | "openai" | "openai-compatible" => return Ok(Backend::OpenAiCompatible),
            _ => writeln!(output, "  Enter 1 for xAI or 2 for OpenAI-compatible.")?,
        }
    }
    bail!("too many invalid backend selections; no changes were made")
}

fn prompt_client<I: PromptInput, W: Write>(
    input: &mut I,
    output: &mut W,
) -> anyhow::Result<ClientChoice> {
    writeln!(output)?;
    writeln!(output, "Generate the next command for:")?;
    writeln!(output, "  1. Claude Code (recommended)")?;
    writeln!(output, "  2. Codex")?;
    writeln!(output, "  3. Skip")?;
    for _ in 0..MAX_ATTEMPTS {
        let value = prompt_line(input, output, "Selection [1]: ")?;
        match value.to_ascii_lowercase().as_str() {
            "" | "1" | "claude" | "claude-code" => return Ok(ClientChoice::ClaudeCode),
            "2" | "codex" => return Ok(ClientChoice::Codex),
            "3" | "skip" => return Ok(ClientChoice::Skip),
            _ => writeln!(
                output,
                "  Enter 1 for Claude Code, 2 for Codex, or 3 to skip."
            )?,
        }
    }
    bail!("too many invalid client selections; no changes were made")
}

fn prompt_url<I: PromptInput, W: Write>(
    input: &mut I,
    output: &mut W,
    label: &str,
    default: Option<&str>,
) -> anyhow::Result<String> {
    for _ in 0..MAX_ATTEMPTS {
        let prompt = match default {
            Some(value) => format!("{label} [{value}]: "),
            None => format!("{label}: "),
        };
        let value = prompt_line(input, output, &prompt)?;
        let candidate = if value.is_empty() {
            match default {
                Some(value) => value.to_string(),
                None => {
                    writeln!(output, "  A URL is required.")?;
                    continue;
                }
            }
        } else {
            value
        };
        match validate_http_url(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(message) => writeln!(output, "  {message}")?,
        }
    }
    bail!("too many invalid URL attempts; no changes were made")
}

fn validate_http_url(value: &str) -> Result<(), &'static str> {
    let parsed = url::Url::parse(value).map_err(|_| "Enter a valid absolute http(s) URL.")?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("URL must use http or https and include a host.");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("URL must not contain embedded credentials.");
    }
    if parsed.query().is_some() {
        return Err("URL must not contain a query string.");
    }
    if parsed.fragment().is_some() {
        return Err("URL must not contain a fragment.");
    }
    Ok(())
}

fn prompt_model<I: PromptInput, W: Write>(
    input: &mut I,
    output: &mut W,
    label: &str,
    default: Option<&str>,
) -> anyhow::Result<String> {
    for _ in 0..MAX_ATTEMPTS {
        let prompt = match default {
            Some(value) => format!("{label} [{value}]: "),
            None => format!("{label}: "),
        };
        let value = prompt_line(input, output, &prompt)?;
        let candidate = if value.is_empty() {
            match default {
                Some(value) => value.to_string(),
                None => {
                    writeln!(output, "  A model is required.")?;
                    continue;
                }
            }
        } else {
            value
        };
        if candidate.chars().any(char::is_whitespace) {
            writeln!(output, "  Model must not contain whitespace.")?;
            continue;
        }
        if candidate.chars().any(char::is_control) {
            writeln!(output, "  Model must not contain control characters.")?;
            continue;
        }
        return Ok(candidate);
    }
    bail!("too many invalid model attempts; no changes were made")
}

fn prompt_line<I: PromptInput, W: Write>(
    input: &mut I,
    output: &mut W,
    prompt: &str,
) -> anyhow::Result<String> {
    write!(output, "{prompt}")?;
    output.flush()?;
    let mut line = String::new();
    if input
        .read_prompt_line(&mut line)
        .context("read setup input")?
        == 0
    {
        bail!("setup input closed; no changes were made")
    }
    Ok(line.trim().to_string())
}

fn prompt_required_secret<S: SecretInput, W: Write>(
    output: &mut W,
    secrets: &mut S,
    label: &str,
) -> anyhow::Result<String> {
    for _ in 0..MAX_ATTEMPTS {
        let value = read_secret(output, secrets, &format!("{label} [hidden]: "))?;
        let value = value.trim().to_string();
        if value.is_empty() {
            writeln!(output, "  A non-empty key is required.")?;
            continue;
        }
        if value.chars().any(char::is_control) {
            writeln!(output, "  Key must not contain control characters.")?;
            continue;
        }
        return Ok(value);
    }
    bail!("too many invalid key attempts; no changes were made")
}

fn prompt_optional_secret<S: SecretInput, W: Write>(
    output: &mut W,
    secrets: &mut S,
    label: &str,
) -> anyhow::Result<Option<String>> {
    for _ in 0..MAX_ATTEMPTS {
        let value = read_secret(output, secrets, &format!("{label} [hidden, optional]: "))?;
        let value = value.trim().to_string();
        if value.is_empty() {
            return Ok(None);
        }
        if value.chars().any(char::is_control) {
            writeln!(output, "  Value must not contain control characters.")?;
            continue;
        }
        return Ok(Some(value));
    }
    bail!("too many invalid secret attempts; no changes were made")
}

fn read_secret<S: SecretInput, W: Write>(
    output: &mut W,
    secrets: &mut S,
    prompt: &str,
) -> anyhow::Result<String> {
    write!(output, "{prompt}")?;
    output.flush()?;
    let value = secrets.read_secret().context("read hidden setup value")?;
    // Hidden terminal readers do not echo or add a newline, so keep subsequent
    // prompts legible. The secret value itself is never written.
    writeln!(output)?;
    Ok(value)
}

fn prompt_yes_no<I: PromptInput, W: Write>(
    input: &mut I,
    output: &mut W,
    question: &str,
    default: bool,
) -> anyhow::Result<bool> {
    let suffix = if default { "[Y/n]" } else { "[y/N]" };
    for _ in 0..MAX_ATTEMPTS {
        let value = prompt_line(input, output, &format!("{question} {suffix}: "))?;
        match value.to_ascii_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => writeln!(output, "  Enter y or n.")?,
        }
    }
    bail!("too many invalid confirmations; no changes were made")
}

fn write_redacted_summary<W: Write>(
    output: &mut W,
    path: &Path,
    answers: &SetupAnswers,
) -> io::Result<()> {
    writeln!(output)?;
    writeln!(output, "Review:")?;
    writeln!(output, "  AI backend:       {}", answers.backend.label())?;
    writeln!(
        output,
        "  API origin:       {}",
        url::Url::parse(&answers.api_url)
            .map(|url| url.origin().ascii_serialization())
            .unwrap_or_else(|_| "<invalid>".to_string())
    )?;
    writeln!(output, "  Model:            {}", answers.model)?;
    writeln!(output, "  AI key:           configured")?;
    writeln!(
        output,
        "  Tavily key:       {}",
        presence(&answers.tavily_api_key)
    )?;
    writeln!(
        output,
        "  Firecrawl key:    {}",
        presence(&answers.firecrawl_api_key)
    )?;
    writeln!(
        output,
        "  GitHub token:     {}",
        presence(&answers.github_token)
    )?;
    writeln!(output, "  Client:           {}", answers.client.label())?;
    writeln!(output, "  Config path:      {}", path.display())?;
    Ok(())
}

fn presence(value: &Option<String>) -> &'static str {
    if value.is_some() {
        "configured"
    } else {
        "not configured"
    }
}

fn render_config(answers: &SetupAnswers) -> anyhow::Result<String> {
    let mut output = String::from("# Generated by `grok-search-rs setup`.\n");
    match answers.backend {
        Backend::XaiResponses => {
            push_toml_string(&mut output, "grok_auth_mode", "api_key")?;
            push_toml_string(&mut output, "grok_api_url", &answers.api_url)?;
            push_toml_string(&mut output, "grok_api_key", &answers.api_key)?;
            push_toml_string(&mut output, "grok_model", &answers.model)?;
        }
        Backend::OpenAiCompatible => {
            push_toml_string(&mut output, "openai_compatible_api_url", &answers.api_url)?;
            push_toml_string(&mut output, "openai_compatible_api_key", &answers.api_key)?;
            push_toml_string(&mut output, "openai_compatible_model", &answers.model)?;
        }
    }
    if let Some(value) = &answers.tavily_api_key {
        push_toml_string(&mut output, "tavily_api_key", value)?;
    }
    if let Some(value) = &answers.firecrawl_api_key {
        push_toml_string(&mut output, "firecrawl_api_key", value)?;
    }
    if let Some(value) = &answers.github_token {
        push_toml_string(&mut output, "github_token", value)?;
    }

    toml::from_str::<toml::Value>(&output).context("validate generated setup TOML")?;
    Ok(output)
}

fn push_toml_string(output: &mut String, key: &str, value: &str) -> anyhow::Result<()> {
    let encoded = serde_json::to_string(value).context("encode TOML string")?;
    output.push_str(key);
    output.push_str(" = ");
    output.push_str(&encoded);
    output.push('\n');
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;
    use std::io::{self, Cursor};

    use tempfile::tempdir;

    use super::*;
    use crate::config::Transport;

    struct FakeSecrets {
        values: VecDeque<String>,
        reads: usize,
    }

    impl FakeSecrets {
        fn new(values: &[&str]) -> Self {
            Self {
                values: values.iter().map(|value| (*value).to_string()).collect(),
                reads: 0,
            }
        }
    }

    impl SecretInput for FakeSecrets {
        fn read_secret(&mut self) -> io::Result<String> {
            self.reads += 1;
            self.values.pop_front().ok_or_else(|| {
                io::Error::new(io::ErrorKind::UnexpectedEof, "no fake secret remaining")
            })
        }
    }

    #[test]
    fn xai_setup_writes_valid_config_without_leaking_secrets() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");
        let mut input = Cursor::new(b"1\n\n\n1\ny\ny\n".to_vec());
        let quoted_key = "xai-quoted-\"-slash-\\";
        let mut secrets = FakeSecrets::new(&["", quoted_key, "tvly-secret", "", "gh-secret"]);
        let mut output = Vec::new();

        let result = run_with_io(true, &path, &mut input, &mut secrets, &mut output).unwrap();

        assert!(result.run_doctor);
        let loaded =
            Config::load_from([("GROK_SEARCH_CONFIG", path.to_string_lossy().into_owned())]);
        assert_eq!(loaded.transport, Transport::Responses);
        assert_eq!(loaded.grok_api_key.as_deref(), Some(quoted_key));
        assert_eq!(
            loaded.grok_api_url, "https://api.x.ai/v1",
            "compiled default must drive setup"
        );
        assert_eq!(loaded.tavily_api_key.as_deref(), Some("tvly-secret"));
        assert_eq!(loaded.firecrawl_api_key, None);
        assert_eq!(loaded.github_token.as_deref(), Some("gh-secret"));

        let transcript = String::from_utf8(output).unwrap();
        for secret in [quoted_key, "tvly-secret", "gh-secret"] {
            assert!(!transcript.contains(secret), "secret leaked: {transcript}");
        }
        assert!(transcript.contains("claude mcp add --scope user grok-search-rs -- grok-search-rs"));
        assert!(transcript.contains(
            "Use grok-search-rs web_search to find the latest Rust MCP SDK release and cite the sources."
        ));
        assert!(transcript.contains("A non-empty key is required."));
    }

    #[test]
    fn openai_setup_retries_invalid_url_and_model_then_writes_codex_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut input = Cursor::new(
            b"2\nhttps://token@gateway.example/v1\nhttps://gateway.example/v1/chat/completions?token=nope\nhttps://gateway.example/v1/chat/completions\nbad model\nsearch-model\n2\ny\n\n"
                .to_vec(),
        );
        let mut secrets = FakeSecrets::new(&["sk-secret", "", "fc-secret", ""]);
        let mut output = Vec::new();

        let result = run_with_io(true, &path, &mut input, &mut secrets, &mut output).unwrap();

        assert!(!result.run_doctor);
        let loaded =
            Config::load_from([("GROK_SEARCH_CONFIG", path.to_string_lossy().into_owned())]);
        assert_eq!(loaded.transport, Transport::ChatCompletions);
        assert_eq!(
            loaded.openai_compatible_api_url.as_deref(),
            Some("https://gateway.example/v1/chat/completions")
        );
        assert_eq!(
            loaded.openai_compatible_api_key.as_deref(),
            Some("sk-secret")
        );
        assert_eq!(
            loaded.openai_compatible_model.as_deref(),
            Some("search-model")
        );
        assert_eq!(loaded.firecrawl_api_key.as_deref(), Some("fc-secret"));
        assert!(loaded.grok_api_key.is_none());

        let transcript = String::from_utf8(output).unwrap();
        assert!(transcript.contains("URL must not contain embedded credentials."));
        assert!(transcript.contains("URL must not contain a query string."));
        assert!(transcript.contains("Model must not contain whitespace."));
        assert!(transcript.contains("codex mcp add grok-search-rs -- grok-search-rs"));
        assert!(!transcript.contains("sk-secret"));
        assert!(!transcript.contains("fc-secret"));
    }

    #[test]
    fn existing_config_is_untouched_and_secrets_are_not_requested() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "sentinel = true\n").unwrap();
        let before = fs::read(&path).unwrap();
        let mut input = Cursor::new(Vec::<u8>::new());
        let mut secrets = FakeSecrets::new(&["must-not-be-read"]);
        let mut output = Vec::new();

        let error = run_with_io(true, &path, &mut input, &mut secrets, &mut output)
            .expect_err("existing file must stop setup");

        assert!(error.to_string().contains("already exists"));
        assert!(error.to_string().contains("grok-search-rs doctor"));
        assert_eq!(secrets.reads, 0);
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn non_tty_fails_before_reading_or_writing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut input = Cursor::new(b"1\n".to_vec());
        let mut secrets = FakeSecrets::new(&["must-not-be-read"]);
        let mut output = Vec::new();

        let error = run_with_io(false, &path, &mut input, &mut secrets, &mut output)
            .expect_err("non-TTY setup must fail");

        assert!(error.to_string().contains("interactive terminal"));
        assert_eq!(secrets.reads, 0);
        assert!(!path.exists());
        assert!(output.is_empty());
    }

    #[test]
    fn declining_confirmation_makes_no_changes_and_skips_doctor() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut input = Cursor::new(b"1\n\n\n3\nn\n".to_vec());
        let mut secrets = FakeSecrets::new(&["xai-secret", "", "", ""]);
        let mut output = Vec::new();

        let result = run_with_io(true, &path, &mut input, &mut secrets, &mut output).unwrap();

        assert_eq!(result, SetupResult { run_doctor: false });
        assert!(!path.exists());
        let transcript = String::from_utf8(output).unwrap();
        assert!(transcript.contains("Setup cancelled. No changes were made."));
        assert!(!transcript.contains("xai-secret"));
        assert!(!transcript.contains("claude mcp add"));
        assert!(!transcript.contains("codex mcp add"));
    }

    #[test]
    fn url_validation_rejects_all_secret_bearing_or_non_base_forms() {
        for value in [
            "ftp://gateway.example/v1",
            "https://user:pass@gateway.example/v1",
            "https://gateway.example/v1?key=secret",
            "https://gateway.example/v1#token",
            "not-a-url",
        ] {
            assert!(
                validate_http_url(value).is_err(),
                "accepted unsafe URL: {value}"
            );
        }
        assert!(validate_http_url("http://localhost:8080/v1").is_ok());
        assert!(validate_http_url("https://gateway.example/v1/responses").is_ok());
    }

    #[test]
    fn redacted_summary_does_not_expose_url_path() {
        let answers = SetupAnswers {
            backend: Backend::OpenAiCompatible,
            api_url: "https://gateway.example/tenant/path-token-canary/v1".to_string(),
            model: "search-model".to_string(),
            api_key: "sk-hidden".to_string(),
            tavily_api_key: None,
            firecrawl_api_key: None,
            github_token: None,
            client: ClientChoice::Skip,
        };
        let mut output = Vec::new();

        write_redacted_summary(&mut output, Path::new("/safe/config.toml"), &answers).unwrap();

        let summary = String::from_utf8(output).unwrap();
        assert!(summary.contains("https://gateway.example"));
        assert!(!summary.contains("path-token-canary"));
        assert!(!summary.contains("sk-hidden"));
    }
}
