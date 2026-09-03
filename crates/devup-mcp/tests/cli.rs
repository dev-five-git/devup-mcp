use std::{ffi::OsString, fs, process::Command};

use devup_mcp::{CliAction, ClientCredentialSource, parse_cli_args, resolve_figma_direct_config};

#[path = "../build_identity.rs"]
mod build_identity;

#[test]
fn build_identity_composes_clean_dirty_and_safe_fallbacks() {
    assert_eq!(
        build_identity::git_identity(Some("0123456789ab"), false).as_deref(),
        Some("0123456789ab")
    );
    assert_eq!(
        build_identity::git_identity(Some("0123456789ab"), true).as_deref(),
        Some("0123456789ab-dirty")
    );
    assert_eq!(
        build_identity::git_identity(Some("unsafe value"), false),
        None
    );
    assert_eq!(build_identity::git_identity(None, false), None);
}

#[test]
fn version_flag_reports_the_installed_binary_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_devup-mcp"))
        .arg("--version")
        .output()
        .expect("run devup-mcp --version");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 version output");
    let prefix = format!("devup-mcp {} (", env!("CARGO_PKG_VERSION"));
    let build_id = stdout
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(")\n"))
        .expect("version output includes a parenthesized build ID");
    assert!(!build_id.is_empty());
    assert!(
        build_id
            .bytes()
            .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') })
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn version_build_id_reports_the_repository_dirty_state() {
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let status = Command::new("git")
        .args(["status", "--porcelain=v1", "--untracked-files=normal"])
        .current_dir(repository)
        .output()
        .expect("inspect repository status");
    assert!(status.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_devup-mcp"))
        .arg("--version")
        .output()
        .expect("run devup-mcp --version");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 version output");
    let build_id = stdout
        .trim_end()
        .rsplit_once('(')
        .and_then(|(_, value)| value.strip_suffix(')'))
        .expect("version output includes a parenthesized build ID");

    assert_eq!(build_id.ends_with("-dirty"), !status.stdout.is_empty());
}

#[test]
fn self_check_is_local_safe_json() -> anyhow::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_devup-mcp"))
        .arg("--self-check")
        .output()
        .expect("run devup-mcp --self-check");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(matches!(report["status"].as_str(), Some("ok" | "degraded")));
    assert_eq!(report["version"], env!("CARGO_PKG_VERSION"));
    assert!(
        report["buildId"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert_eq!(report["binary"], "ok");
    assert!(matches!(
        report["credentialBackend"].as_str(),
        Some("ok" | "unavailable")
    ));
    assert_eq!(report["serverConfig"], "ok");
    let serialized = report.to_string();
    assert!(!serialized.contains(&std::env::current_dir()?.display().to_string()));
    Ok(())
}

#[test]
fn repeated_allowed_write_roots_are_validated_before_stdio() -> anyhow::Result<()> {
    let base = std::env::temp_dir().join(format!("devup-mcp-cli-roots-{}", std::process::id()));
    let first = base.join("first");
    let second = base.join("second");
    fs::create_dir_all(&first)?;
    fs::create_dir_all(&second)?;

    let action = parse_cli_args([
        OsString::from("--allow-write-root"),
        first.clone().into_os_string(),
        OsString::from("--allow-write-root"),
        second.clone().into_os_string(),
    ])?;

    let CliAction::Serve(config) = action else {
        panic!("write roots must start the server")
    };
    assert_eq!(config.allowed_write_roots, vec![first, second]);

    fs::remove_dir_all(base)?;
    Ok(())
}

#[test]
fn invalid_cli_arguments_fail_without_starting_stdio() -> anyhow::Result<()> {
    let file = std::env::temp_dir().join(format!("devup-mcp-cli-file-{}", std::process::id()));
    fs::write(&file, b"not a directory")?;

    assert!(parse_cli_args([OsString::from("--unknown")]).is_err());
    assert!(parse_cli_args([OsString::from("--allow-write-root")]).is_err());
    assert!(
        parse_cli_args([
            OsString::from("--allow-write-root"),
            file.clone().into_os_string()
        ])
        .is_err()
    );

    fs::remove_file(file)?;
    Ok(())
}

#[test]
fn no_arguments_use_the_startup_current_directory() -> anyhow::Result<()> {
    let action = parse_cli_args(std::iter::empty::<OsString>())?;
    let CliAction::Serve(config) = action else {
        panic!("no arguments must start the server")
    };
    assert_eq!(config.allowed_write_roots, vec![std::env::current_dir()?]);
    assert_eq!(config.figma_client_id, None);
    assert_eq!(config.figma_client_secret, None);
    assert_eq!(config.figma_callback_port, None);
    assert_eq!(config.figma_client_name, None);
    Ok(())
}

#[test]
fn figma_client_credential_and_callback_port_flags_populate_server_config() -> anyhow::Result<()> {
    let action = parse_cli_args([
        OsString::from("--figma-client-id"),
        OsString::from("preregistered-client"),
        OsString::from("--figma-client-secret"),
        OsString::from("preregistered-secret"),
        OsString::from("--figma-callback-port"),
        OsString::from("19876"),
    ])?;
    let CliAction::Serve(config) = action else {
        panic!("figma flags must start the server")
    };
    assert_eq!(
        config.figma_client_id.as_deref(),
        Some("preregistered-client")
    );
    assert_eq!(
        config.figma_client_secret.as_deref(),
        Some("preregistered-secret")
    );
    assert_eq!(config.figma_callback_port, Some(19876));
    Ok(())
}

#[test]
fn figma_callback_port_rejects_missing_or_non_numeric_values() {
    assert!(parse_cli_args([OsString::from("--figma-callback-port")]).is_err());
    assert!(
        parse_cli_args([
            OsString::from("--figma-callback-port"),
            OsString::from("not-a-port"),
        ])
        .is_err()
    );
    assert!(
        parse_cli_args([
            OsString::from("--figma-callback-port"),
            OsString::from("70000"),
        ])
        .is_err(),
        "70000 exceeds u16::MAX and must be rejected, not silently truncated"
    );
}

#[test]
fn figma_client_id_and_secret_reject_missing_or_empty_values() {
    assert!(parse_cli_args([OsString::from("--figma-client-id")]).is_err());
    assert!(parse_cli_args([OsString::from("--figma-client-secret")]).is_err());
    assert!(parse_cli_args([OsString::from("--figma-client-id"), OsString::from("")]).is_err());
    assert!(parse_cli_args([OsString::from("--figma-client-secret"), OsString::from("")]).is_err());
}

/// The DCR `client_name` is what Figma's catalog allowlist is matched
/// against, so it is configurable at launch. It is trimmed, and a blank
/// value is an error rather than a silently-sent empty identity.
#[test]
fn figma_client_name_flag_populates_server_config_and_rejects_blank_values() -> anyhow::Result<()> {
    let action = parse_cli_args([
        OsString::from("--figma-client-name"),
        OsString::from("  Acme Registered Client  "),
    ])?;
    let CliAction::Serve(config) = action else {
        panic!("--figma-client-name must start the server")
    };
    assert_eq!(
        config.figma_client_name.as_deref(),
        Some("Acme Registered Client")
    );

    assert!(parse_cli_args([OsString::from("--figma-client-name")]).is_err());
    assert!(parse_cli_args([OsString::from("--figma-client-name"), OsString::from("")]).is_err());
    assert!(
        parse_cli_args([OsString::from("--figma-client-name"), OsString::from("   ")]).is_err()
    );
    Ok(())
}

#[test]
fn version_and_self_check_are_rejected_when_combined_with_figma_flags() {
    // `--version`/`--self-check` must only win when they are the *sole*
    // argument; combined with a figma flag they must not silently swallow
    // the other flag and report a stale version/self-check instead of an
    // error.
    assert!(
        parse_cli_args([
            OsString::from("--figma-client-id"),
            OsString::from("preregistered-client"),
            OsString::from("--self-check"),
        ])
        .is_err()
    );
    assert!(
        parse_cli_args([
            OsString::from("--figma-client-id"),
            OsString::from("preregistered-client"),
            OsString::from("--version"),
        ])
        .is_err()
    );
}

#[test]
fn resolve_figma_direct_config_prioritizes_cli_arg_over_env() {
    let resolved = resolve_figma_direct_config(
        Some("cli-client".to_owned()),
        Some("cli-secret".to_owned()),
        Some(19876),
        Some("Cli Client Name".to_owned()),
        Some("env-client".to_owned()),
        Some("env-secret".to_owned()),
        Some("Env Client Name".to_owned()),
    );
    assert_eq!(resolved.client_id.as_deref(), Some("cli-client"));
    assert_eq!(resolved.client_secret.as_deref(), Some("cli-secret"));
    assert_eq!(resolved.credential_source, ClientCredentialSource::CliArg);
    assert_eq!(resolved.callback_port, Some(19876));
    assert_eq!(resolved.client_name.as_deref(), Some("Cli Client Name"));
}

#[test]
fn resolve_figma_direct_config_falls_back_to_env_then_to_none() {
    let env_only = resolve_figma_direct_config(
        None,
        None,
        None,
        None,
        Some("env-client".to_owned()),
        Some("env-secret".to_owned()),
        Some("Env Client Name".to_owned()),
    );
    assert_eq!(env_only.client_id.as_deref(), Some("env-client"));
    assert_eq!(env_only.credential_source, ClientCredentialSource::Env);
    assert_eq!(env_only.client_name.as_deref(), Some("Env Client Name"));

    let neither = resolve_figma_direct_config(None, None, None, None, None, None, None);
    assert_eq!(neither.client_id, None);
    assert_eq!(neither.client_secret, None);
    assert_eq!(neither.credential_source, ClientCredentialSource::None);
    assert_eq!(neither.client_name, None);

    // Callback port is independent of credential source: it always comes
    // from the cli-arg value regardless of which credential source won.
    let callback_port_only =
        resolve_figma_direct_config(None, None, Some(19876), None, None, None, None);
    assert_eq!(callback_port_only.callback_port, Some(19876));
    assert_eq!(
        callback_port_only.credential_source,
        ClientCredentialSource::None
    );
}

/// The client name lives on the Dynamic Client Registration path, which a
/// pre-registered `client_id` skips outright — so it must resolve
/// independently of the credential pair, and be available even when no
/// credential is configured at all (exactly the case where DCR runs).
#[test]
fn resolve_figma_direct_config_resolves_client_name_independently_of_credentials() {
    let name_without_credentials = resolve_figma_direct_config(
        None,
        None,
        None,
        Some("Acme Registered Client".to_owned()),
        None,
        None,
        None,
    );
    assert_eq!(
        name_without_credentials.client_name.as_deref(),
        Some("Acme Registered Client")
    );
    assert_eq!(name_without_credentials.client_id, None);
    assert_eq!(
        name_without_credentials.credential_source,
        ClientCredentialSource::None
    );

    // No cli-arg name: the env value carries even when the winning
    // credential source is the cli arg.
    let env_name_with_cli_credentials = resolve_figma_direct_config(
        Some("cli-client".to_owned()),
        None,
        None,
        None,
        None,
        None,
        Some("Env Client Name".to_owned()),
    );
    assert_eq!(
        env_name_with_cli_credentials.client_name.as_deref(),
        Some("Env Client Name")
    );
    assert_eq!(
        env_name_with_cli_credentials.credential_source,
        ClientCredentialSource::CliArg
    );
}
