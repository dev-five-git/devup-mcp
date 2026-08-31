use std::process::Command;

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
