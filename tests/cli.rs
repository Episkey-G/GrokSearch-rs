use std::fs;
use std::process::{Command, Stdio};

use serde_json::Value;
use tempfile::tempdir;

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_grok-search-rs"))
}

fn has_check_code(report: &Value, code: &str) -> bool {
    report["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|check| check["code"] == code)
}

#[test]
fn doctor_json_reports_missing_credentials_without_network() {
    let home = tempdir().unwrap();
    let output = command()
        .args(["doctor", "--json"])
        .env_clear()
        .env("HOME", home.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "doctor stdout must be one JSON document: {err}; stdout={}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["ok"], false);
    assert_eq!(report["status"], "not_ready");
    assert!(has_check_code(&report, "missing_credentials"));
}

#[test]
fn doctor_human_output_reports_not_ready_and_exits_non_zero() {
    let home = tempdir().unwrap();
    let output = command()
        .arg("doctor")
        .env_clear()
        .env("HOME", home.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "the human path must use the same non-zero exit code as --json"
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.starts_with("grok-search-rs doctor v"),
        "unexpected human report header: {stdout}"
    );
    assert!(stdout.contains("Status: NOT READY"), "{stdout}");
    assert!(stdout.contains("(missing_credentials)"), "{stdout}");
    assert!(
        serde_json::from_str::<Value>(&stdout).is_err(),
        "the human path must not emit the --json document"
    );
}

#[test]
fn doctor_rejects_unknown_arguments_instead_of_exiting_zero() {
    // A deploy gate reads the exit code, so an unrecognized argument shape must
    // fail loudly instead of being treated as a healthy run. `-v` is included
    // because the match-anywhere version flag must not shadow a subcommand's
    // own argument validation.
    for args in [
        vec!["doctor", "--bogus"],
        vec!["doctor", "--json", "--json"],
        vec!["doctor", "-v"],
    ] {
        let home = tempdir().unwrap();
        let output = command()
            .args(&args)
            .env_clear()
            .env("HOME", home.path())
            .output()
            .unwrap();

        assert!(!output.status.success(), "{args:?} must not exit 0");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(
            stderr.contains("usage: grok-search-rs doctor [--json]"),
            "{args:?} did not report the doctor usage: {stderr}"
        );
        assert!(
            !stdout.contains(env!("CARGO_PKG_VERSION")),
            "{args:?} printed the version instead of a usage error"
        );
    }
}

#[test]
fn doctor_json_does_not_echo_secrets_from_malformed_config() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let canary = "xai-cli-doctor-secret-canary";
    fs::write(
        &config_path,
        format!("grok_api_key = \"{canary}\"\nunknown_field = true\n"),
    )
    .unwrap();

    let output = command()
        .args(["doctor", "--json"])
        .env_clear()
        .env("GROK_SEARCH_CONFIG", &config_path)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let report: Value = serde_json::from_str(&stdout).unwrap();
    assert!(has_check_code(&report, "invalid_config_file"));
    assert!(
        !stdout.contains(canary),
        "doctor leaked a configured secret"
    );
}

#[test]
fn doctor_escapes_control_characters_in_invalid_auth_mode_warning() {
    let home = tempdir().unwrap();
    let output = command()
        .args(["doctor", "--json"])
        .env_clear()
        .env("HOME", home.path())
        .env("GROK_SEARCH_AUTH_MODE", "bad\nmode")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(r#"GROK_SEARCH_AUTH_MODE="bad\nmode""#));
    assert!(!stderr.contains("bad\nmode"));
}

#[test]
fn setup_refuses_non_terminal_input_without_creating_a_file() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let output = command()
        .arg("setup")
        .env_clear()
        .env("GROK_SEARCH_CONFIG", &config_path)
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(!config_path.exists());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.to_ascii_lowercase().contains("interactive"),
        "expected an actionable TTY error, got: {combined}"
    );
}
