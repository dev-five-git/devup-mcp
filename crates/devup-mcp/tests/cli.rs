use std::{ffi::OsString, fs, process::Command};

use devup_mcp::{CliAction, parse_cli_args};

#[test]
fn version_flag_reports_the_installed_binary_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_devup-mcp"))
        .arg("--version")
        .output()
        .expect("run devup-mcp --version");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 version output"),
        format!("devup-mcp {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
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
