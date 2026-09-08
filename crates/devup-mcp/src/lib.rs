pub mod server;

use std::{ffi::OsString, path::PathBuf};

use serde::Serialize;

pub use devup_mcp_figma::ClientCredentialSource;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub allowed_write_roots: Vec<PathBuf>,
    /// From `--figma-client-id`. `None` unless the flag was passed.
    pub figma_client_id: Option<String>,
    /// From `--figma-client-secret`. `None` unless the flag was passed.
    pub figma_client_secret: Option<String>,
    /// From `--figma-callback-port`. `None` preserves the pre-existing
    /// OS-assigned-port behavior.
    pub figma_callback_port: Option<u16>,
    /// From `--figma-client-name`. `None` keeps devup-mcp's own literal
    /// name for Dynamic Client Registration.
    pub figma_client_name: Option<String>,
}

/// Fully resolved Figma direct-connection configuration: cli-arg values
/// (if any) win over environment variables, which win over "nothing
/// configured here" (the persisted `configure` store, if any, is resolved
/// later inside `OAuthManager`, not here). Built by
/// [`resolve_figma_direct_config`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FigmaDirectConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub credential_source: ClientCredentialSource,
    pub callback_port: Option<u16>,
    /// `client_name` for Dynamic Client Registration. `None` keeps
    /// [`devup_mcp_figma::DEFAULT_CLIENT_NAME`]. Resolved independently of
    /// the client-id/secret pair: a pre-registered credential skips DCR
    /// entirely, so the two settings are never both in play.
    pub client_name: Option<String>,
}

/// Resolves the effective Figma direct-connection client credential from
/// (in priority order) cli-arg flags, then environment variables. Takes
/// the environment values as explicit parameters — rather than reading
/// `std::env::var` internally — so this stays a pure, deterministically
/// testable function; callers pass real env values at the process
/// boundary (see `run_stdio_with_config`, `self_check`).
pub fn resolve_figma_direct_config(
    cli_client_id: Option<String>,
    cli_client_secret: Option<String>,
    cli_callback_port: Option<u16>,
    cli_client_name: Option<String>,
    env_client_id: Option<String>,
    env_client_secret: Option<String>,
    env_client_name: Option<String>,
) -> FigmaDirectConfig {
    // Resolved independently of the credential pair below: a client name
    // only matters on the Dynamic Client Registration path, which a
    // pre-registered client_id skips outright.
    let client_name = cli_client_name.or(env_client_name);
    if let Some(client_id) = cli_client_id {
        return FigmaDirectConfig {
            client_id: Some(client_id),
            client_secret: cli_client_secret,
            credential_source: ClientCredentialSource::CliArg,
            callback_port: cli_callback_port,
            client_name,
        };
    }
    if let Some(client_id) = env_client_id {
        return FigmaDirectConfig {
            client_id: Some(client_id),
            client_secret: env_client_secret,
            credential_source: ClientCredentialSource::Env,
            callback_port: cli_callback_port,
            client_name,
        };
    }
    FigmaDirectConfig {
        callback_port: cli_callback_port,
        client_name,
        ..FigmaDirectConfig::default()
    }
}

/// Reads `DEVUP_FIGMA_CLIENT_ID`/`DEVUP_FIGMA_CLIENT_SECRET`/
/// `DEVUP_FIGMA_CLIENT_NAME`, treating an empty value the same as an
/// unset one.
fn env_figma_client_credentials() -> (Option<String>, Option<String>, Option<String>) {
    let read = |key: &str| {
        std::env::var(key)
            .ok()
            .filter(|value| !value.trim().is_empty())
    };
    (
        read("DEVUP_FIGMA_CLIENT_ID"),
        read("DEVUP_FIGMA_CLIENT_SECRET"),
        read("DEVUP_FIGMA_CLIENT_NAME"),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Version,
    SelfCheck,
    Serve(ServerConfig),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfCheckReport {
    pub status: &'static str,
    pub version: &'static str,
    pub build_id: &'static str,
    pub binary: &'static str,
    pub credential_backend: &'static str,
    pub server_config: &'static str,
}

pub const fn build_id() -> &'static str {
    env!("DEVUP_MCP_BUILD_ID")
}

pub fn parse_cli_args<I, T>(arguments: I) -> anyhow::Result<CliAction>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut arguments = arguments.into_iter().map(Into::into).peekable();
    let mut roots = Vec::new();
    let mut figma_client_id: Option<String> = None;
    let mut figma_client_secret: Option<String> = None;
    let mut figma_callback_port: Option<u16> = None;
    let mut figma_client_name: Option<String> = None;
    while let Some(argument) = arguments.next() {
        let no_other_options_yet = roots.is_empty()
            && figma_client_id.is_none()
            && figma_client_secret.is_none()
            && figma_callback_port.is_none()
            && figma_client_name.is_none();
        match argument.to_str() {
            Some("--version" | "-V") if no_other_options_yet && arguments.peek().is_none() => {
                return Ok(CliAction::Version);
            }
            Some("--self-check") if no_other_options_yet && arguments.peek().is_none() => {
                return Ok(CliAction::SelfCheck);
            }
            Some("--allow-write-root") => {
                let root = arguments.next().ok_or_else(|| {
                    anyhow::anyhow!("--allow-write-root requires a directory path.")
                })?;
                let root = PathBuf::from(root);
                if !root.is_dir() {
                    anyhow::bail!("--allow-write-root must be an existing directory.");
                }
                roots.push(root);
            }
            Some("--figma-client-id") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--figma-client-id requires a value."))?;
                let value = value
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("--figma-client-id must be a UTF-8 string."))?
                    .to_owned();
                if value.is_empty() {
                    anyhow::bail!("--figma-client-id must not be empty.");
                }
                figma_client_id = Some(value);
            }
            Some("--figma-client-secret") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--figma-client-secret requires a value."))?;
                let value = value
                    .to_str()
                    .ok_or_else(|| {
                        anyhow::anyhow!("--figma-client-secret must be a UTF-8 string.")
                    })?
                    .to_owned();
                if value.is_empty() {
                    anyhow::bail!("--figma-client-secret must not be empty.");
                }
                figma_client_secret = Some(value);
            }
            Some("--figma-client-name") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--figma-client-name requires a value."))?;
                let value = value
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("--figma-client-name must be a UTF-8 string."))?
                    .trim()
                    .to_owned();
                if value.is_empty() {
                    anyhow::bail!("--figma-client-name must not be empty.");
                }
                figma_client_name = Some(value);
            }
            Some("--figma-callback-port") => {
                let value = arguments.next().ok_or_else(|| {
                    anyhow::anyhow!("--figma-callback-port requires a port number.")
                })?;
                let value = value.to_str().ok_or_else(|| {
                    anyhow::anyhow!("--figma-callback-port must be a UTF-8 string.")
                })?;
                figma_callback_port = Some(value.parse::<u16>().map_err(|_| {
                    anyhow::anyhow!("--figma-callback-port must be a number between 1 and 65535.")
                })?);
            }
            Some(flag) => anyhow::bail!("Unsupported devup-mcp argument: {flag}"),
            None => anyhow::bail!("devup-mcp arguments must be UTF-8 flags."),
        }
    }
    if roots.is_empty() {
        roots.push(std::env::current_dir()?);
    }
    Ok(CliAction::Serve(ServerConfig {
        allowed_write_roots: roots,
        figma_client_id,
        figma_client_secret,
        figma_callback_port,
        figma_client_name,
    }))
}

pub fn self_check() -> SelfCheckReport {
    let credential_ok = devup_mcp_figma::KeyringCredentialStore::probe().is_ok();
    let (env_client_id, env_client_secret, env_client_name) = env_figma_client_credentials();
    let figma_direct = resolve_figma_direct_config(
        None,
        None,
        None,
        None,
        env_client_id,
        env_client_secret,
        env_client_name,
    );
    let server_ok = std::env::current_dir()
        .ok()
        .and_then(|root| server::DevupServer::production_with_config(vec![root], figma_direct).ok())
        .is_some();
    SelfCheckReport {
        status: if credential_ok && server_ok {
            "ok"
        } else {
            "degraded"
        },
        version: env!("CARGO_PKG_VERSION"),
        build_id: build_id(),
        binary: "ok",
        credential_backend: if credential_ok { "ok" } else { "unavailable" },
        server_config: if server_ok { "ok" } else { "unavailable" },
    }
}

pub async fn run_stdio() -> anyhow::Result<()> {
    let CliAction::Serve(config) = parse_cli_args(std::iter::empty::<OsString>())? else {
        unreachable!("empty arguments always start the server")
    };
    run_stdio_with_config(config).await
}

pub async fn run_stdio_with_config(config: ServerConfig) -> anyhow::Result<()> {
    use rmcp::ServiceExt;

    let (env_client_id, env_client_secret, env_client_name) = env_figma_client_credentials();
    let figma_direct = resolve_figma_direct_config(
        config.figma_client_id.clone(),
        config.figma_client_secret.clone(),
        config.figma_callback_port,
        config.figma_client_name.clone(),
        env_client_id,
        env_client_secret,
        env_client_name,
    );
    let service =
        server::DevupServer::production_with_config(config.allowed_write_roots, figma_direct)?
            .serve((tokio::io::stdin(), tokio::io::stdout()))
            .await?;
    service.waiting().await?;
    Ok(())
}
