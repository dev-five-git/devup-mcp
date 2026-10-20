use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::Serialize;
use serde_json::{Map, Value, json};

const SERVER_NAME: &str = "devup-mcp";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    pub binary: BinaryReport,
    pub clients: Vec<ClientReport>,
    pub next: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryReport {
    pub path: PathBuf,
    pub action: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientReport {
    pub client: &'static str,
    pub config: PathBuf,
    pub action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

pub fn home_dir() -> anyhow::Result<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .context("Could not determine the home directory to install into.")
}

/// Outside every package manager directory, so the release check keeps this
/// copy current (`self_update` treats it as `owned`).
pub fn default_binary_path(home: &Path) -> PathBuf {
    if cfg!(windows) {
        home.join("AppData")
            .join("Local")
            .join("devup-mcp")
            .join("bin")
            .join("devup-mcp.exe")
    } else {
        home.join(".local").join("bin").join("devup-mcp")
    }
}

pub fn install() -> anyhow::Result<InstallReport> {
    let home = home_dir()?;
    let source =
        std::env::current_exe().context("Could not locate the running devup-mcp binary.")?;
    install_at(&home, &source, &default_binary_path(&home))
}

pub fn install_at(home: &Path, source: &Path, target: &Path) -> anyhow::Result<InstallReport> {
    let binary = BinaryReport {
        path: target.to_path_buf(),
        action: copy_binary(source, target)?,
    };
    let command = target.to_string_lossy().into_owned();
    Ok(InstallReport {
        binary,
        clients: vec![
            register_opencode(home, &command),
            register_claude_code(home, &command),
            register_codex(home, &command),
        ],
        next: "Restart each client, or reconnect its MCP servers, so it starts the installed binary.",
    })
}

fn copy_binary(source: &Path, target: &Path) -> anyhow::Result<&'static str> {
    if let (Ok(a), Ok(b)) = (dunce::canonicalize(source), dunce::canonicalize(target))
        && a == b
    {
        return Ok("already-installed");
    }
    let bytes =
        fs::read(source).with_context(|| format!("Could not read {}.", source.display()))?;
    if fs::read(target).is_ok_and(|existing| existing == bytes) {
        return Ok("unchanged");
    }
    let parent = target
        .parent()
        .context("The install path has no parent directory.")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("Could not create {}.", parent.display()))?;
    let staged = sibling(target, ".install");
    fs::write(&staged, &bytes).with_context(|| format!("Could not write {}.", staged.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
    }
    if target.exists() {
        // A running image cannot be overwritten on Windows but can be renamed,
        // so a client still holding the old binary does not block the install.
        let previous = sibling(target, ".previous");
        match fs::remove_file(&previous) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                return Err(error)
                    .with_context(|| format!("Could not remove {}.", previous.display()));
            }
            _ => {}
        }
        fs::rename(target, &previous)
            .with_context(|| format!("Could not move aside {}.", target.display()))?;
    }
    fs::rename(&staged, target)
        .with_context(|| format!("Could not place {}.", target.display()))?;
    Ok("copied")
}

fn register_opencode(home: &Path, command: &str) -> ClientReport {
    let dir = home.join(".config").join("opencode");
    let config = ["opencode.jsonc", "opencode.json"]
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| dir.join("opencode.json"));
    if !dir.is_dir() {
        return skipped("opencode", config, "opencode is not set up for this user.");
    }
    let result = update_json(&config, |root| {
        let entry = object_at(object_at(root, "mcp")?, SERVER_NAME)?;
        entry.insert("type".into(), json!("local"));
        entry.insert("command".into(), json!([command]));
        entry.insert("enabled".into(), json!(true));
        Ok(())
    });
    report("opencode", config, result)
}

fn register_claude_code(home: &Path, command: &str) -> ClientReport {
    let config = home.join(".claude.json");
    if !config.is_file() && !home.join(".claude").is_dir() {
        return skipped(
            "claude-code",
            config,
            "Claude Code is not set up for this user.",
        );
    }
    let result = update_json(&config, |root| {
        let entry = object_at(object_at(root, "mcpServers")?, SERVER_NAME)?;
        entry.insert("type".into(), json!("stdio"));
        entry.insert("command".into(), json!(command));
        entry.insert("args".into(), json!([]));
        Ok(())
    });
    report("claude-code", config, result)
}

fn register_codex(home: &Path, command: &str) -> ClientReport {
    let dir = home.join(".codex");
    let config = dir.join("config.toml");
    if !dir.is_dir() {
        return skipped("codex", config, "Codex is not set up for this user.");
    }
    let result = update_codex(&config, command);
    report("codex", config, result)
}

fn update_codex(path: &Path, command: &str) -> anyhow::Result<&'static str> {
    let original = read_optional(path)?;
    let mut document: toml_edit::DocumentMut = original
        .as_deref()
        .unwrap_or("")
        .parse()
        .with_context(|| format!("{} is not valid TOML.", path.display()))?;
    if !document.contains_key("mcp_servers") {
        let mut servers = toml_edit::Table::new();
        servers.set_implicit(true);
        document.insert("mcp_servers", toml_edit::Item::Table(servers));
    }
    let servers = document["mcp_servers"]
        .as_table_mut()
        .context("`mcp_servers` in the Codex config is not a table.")?;
    let entry = servers
        .entry(SERVER_NAME)
        .or_insert(toml_edit::table())
        .as_table_mut()
        .context("`mcp_servers.devup-mcp` in the Codex config is not a table.")?;
    let mut changed = false;
    if entry.get("command").and_then(toml_edit::Item::as_str) != Some(command) {
        entry.insert("command", toml_edit::value(command));
        changed = true;
    }
    let args_empty = entry
        .get("args")
        .and_then(toml_edit::Item::as_array)
        .is_some_and(toml_edit::Array::is_empty);
    if !args_empty {
        entry.insert("args", toml_edit::value(toml_edit::Array::new()));
        changed = true;
    }
    if !changed {
        return Ok("unchanged");
    }
    write_config(path, original.as_deref(), &document.to_string())
}

fn update_json(
    path: &Path,
    edit: impl FnOnce(&mut Map<String, Value>) -> anyhow::Result<()>,
) -> anyhow::Result<&'static str> {
    let original = read_optional(path)?;
    let mut root: Value = match original.as_deref() {
        Some(text) if !text.trim().is_empty() => serde_json::from_str(text).with_context(|| {
            format!(
                "{} is not plain JSON (comments or trailing commas?). Add the devup-mcp entry by hand.",
                path.display()
            )
        })?,
        _ => json!({}),
    };
    let before = root.clone();
    edit(
        root.as_object_mut()
            .context("The config's top level is not a JSON object.")?,
    )?;
    if root == before {
        return Ok("unchanged");
    }
    let contents = format!("{}\n", serde_json::to_string_pretty(&root)?);
    write_config(path, original.as_deref(), &contents)
}

fn object_at<'a>(
    map: &'a mut Map<String, Value>,
    key: &str,
) -> anyhow::Result<&'a mut Map<String, Value>> {
    map.entry(key)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .with_context(|| format!("`{key}` in the config is not an object."))
}

fn read_optional(path: &Path) -> anyhow::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Could not read {}.", path.display())),
    }
}

/// The first edit keeps the untouched original beside the config, so the
/// pre-devup-mcp state is always one rename away.
fn write_config(
    path: &Path,
    original: Option<&str>,
    contents: &str,
) -> anyhow::Result<&'static str> {
    if let Some(original) = original {
        let backup = sibling(path, ".devup-mcp.bak");
        if !backup.exists() {
            fs::write(&backup, original)
                .with_context(|| format!("Could not write {}.", backup.display()))?;
        }
    } else if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let staged = sibling(path, ".devup-mcp.tmp");
    fs::write(&staged, contents)
        .with_context(|| format!("Could not write {}.", staged.display()))?;
    fs::rename(&staged, path).with_context(|| format!("Could not replace {}.", path.display()))?;
    Ok(if original.is_some() {
        "updated"
    } else {
        "created"
    })
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

fn skipped(client: &'static str, config: PathBuf, reason: &str) -> ClientReport {
    ClientReport {
        client,
        config,
        action: "skipped",
        reason: Some(reason.to_owned()),
    }
}

fn report(
    client: &'static str,
    config: PathBuf,
    result: anyhow::Result<&'static str>,
) -> ClientReport {
    match result {
        Ok(action) => ClientReport {
            client,
            config,
            action,
            reason: None,
        },
        Err(error) => ClientReport {
            client,
            config,
            action: "failed",
            reason: Some(format!("{error:#}")),
        },
    }
}
