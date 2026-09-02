use std::{ffi::OsString, fs, process::Command};

use devup_mcp::{CliAction, parse_cli_args};

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
    Ok(())
}
