use std::fs;

use devup_mcp::install::install_at;

#[test]
fn install_copies_the_binary_and_registers_every_set_up_client() -> anyhow::Result<()> {
    let base = std::env::temp_dir().join(format!("devup-mcp-install-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let home = base.join("home");
    fs::create_dir_all(home.join(".config").join("opencode"))?;
    fs::create_dir_all(home.join(".codex"))?;
    fs::write(
        home.join(".config/opencode/opencode.json"),
        r#"{"mcp":{"devup-mcp":{"type":"local","command":["python","launcher.py"],"timeout":30000},"other":{"type":"remote"}}}"#,
    )?;
    fs::write(
        home.join(".codex/config.toml"),
        "model = \"x\"\n\n[mcp_servers.devup-mcp]\ncommand = 'python.exe'\nargs = ['launcher.py']\nstartup_timeout_sec = 30.0\n",
    )?;
    let source = base.join("source-bin");
    fs::write(&source, b"binary-v1")?;
    let target = base.join("bin").join("devup-mcp");

    let report = install_at(&home, &source, &target)?;
    assert_eq!(report.binary.action, "copied");
    assert_eq!(fs::read(&target)?, b"binary-v1");
    let command = target.to_string_lossy().into_owned();

    let opencode: serde_json::Value = serde_json::from_str(&fs::read_to_string(
        home.join(".config/opencode/opencode.json"),
    )?)?;
    assert_eq!(
        opencode["mcp"]["devup-mcp"]["command"],
        serde_json::json!([command])
    );
    assert_eq!(opencode["mcp"]["devup-mcp"]["timeout"], 30000);
    assert_eq!(opencode["mcp"]["other"]["type"], "remote");
    assert!(
        home.join(".config/opencode/opencode.json.devup-mcp.bak")
            .is_file()
    );

    let codex: toml_edit::DocumentMut =
        fs::read_to_string(home.join(".codex/config.toml"))?.parse()?;
    assert_eq!(codex["model"].as_str(), Some("x"));
    assert_eq!(
        codex["mcp_servers"]["devup-mcp"]["command"].as_str(),
        Some(command.as_str())
    );
    assert!(
        codex["mcp_servers"]["devup-mcp"]["args"]
            .as_array()
            .is_some_and(|a| a.is_empty())
    );
    assert_eq!(
        codex["mcp_servers"]["devup-mcp"]["startup_timeout_sec"].as_float(),
        Some(30.0)
    );

    let claude = report
        .clients
        .iter()
        .find(|c| c.client == "claude-code")
        .unwrap();
    assert_eq!(claude.action, "skipped");

    let again = install_at(&home, &source, &target)?;
    assert_eq!(again.binary.action, "unchanged");
    assert!(
        again
            .clients
            .iter()
            .filter(|c| c.client != "claude-code")
            .all(|c| c.action == "unchanged")
    );

    fs::write(&source, b"binary-v2")?;
    assert_eq!(install_at(&home, &source, &target)?.binary.action, "copied");
    assert_eq!(fs::read(&target)?, b"binary-v2");

    fs::remove_dir_all(base)?;
    Ok(())
}

#[test]
fn jsonc_config_is_reported_not_rewritten() -> anyhow::Result<()> {
    let base = std::env::temp_dir().join(format!("devup-mcp-install-jsonc-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let home = base.join("home");
    let dir = home.join(".config").join("opencode");
    fs::create_dir_all(&dir)?;
    let original = "{\n  // keep me\n  \"mcp\": {}\n}\n";
    fs::write(dir.join("opencode.jsonc"), original)?;
    let source = base.join("source-bin");
    fs::write(&source, b"bin")?;

    let report = install_at(&home, &source, &base.join("bin").join("devup-mcp"))?;
    let opencode = report
        .clients
        .iter()
        .find(|c| c.client == "opencode")
        .unwrap();
    assert_eq!(opencode.action, "failed");
    assert_eq!(fs::read_to_string(dir.join("opencode.jsonc"))?, original);

    fs::remove_dir_all(base)?;
    Ok(())
}
