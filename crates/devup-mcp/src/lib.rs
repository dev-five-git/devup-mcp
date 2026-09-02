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
    env_client_id: Option<String>,
    env_client_secret: Option<String>,
) -> FigmaDirectConfig {
    if let Some(client_id) = cli_client_id {
        return FigmaDirectConfig {
            client_id: Some(client_id),
            client_secret: cli_client_secret,
            credential_source: ClientCredentialSource::CliArg,
            callback_port: cli_callback_port,
        };
    }
    if let Some(client_id) = env_client_id {
        return FigmaDirectConfig {
            client_id: Some(client_id),
            client_secret: env_client_secret,
            credential_source: ClientCredentialSource::Env,
            callback_port: cli_callback_port,
        };
    }
    FigmaDirectConfig {
        callback_port: cli_callback_port,
        ..FigmaDirectConfig::default()
    }
}

/// Reads `DEVUP_FIGMA_CLIENT_ID`/`DEVUP_FIGMA_CLIENT_SECRET`, treating an
/// empty value the same as an unset one.
fn env_figma_client_credentials() -> (Option<String>, Option<String>) {
    let client_id = std::env::var("DEVUP_FIGMA_CLIENT_ID")
        .ok()
        .filter(|value| !value.is_empty());
    let client_secret = std::env::var("DEVUP_FIGMA_CLIENT_SECRET")
        .ok()
        .filter(|value| !value.is_empty());
    (client_id, client_secret)
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
    while let Some(argument) = arguments.next() {
        let no_other_options_yet = roots.is_empty()
            && figma_client_id.is_none()
            && figma_client_secret.is_none()
            && figma_callback_port.is_none();
        match argument.to_str() {
            Some("--version" | "-V") if no_other_options_yet && arguments.peek().is_none() => {
                return Ok(CliAction::Version);
            }
            Some("--self-check") if no_other_options_yet && arguments.peek().is_none() => {
                return Ok(CliAction::SelfCheck);
            }
            Some("--allow-write-root") => {
                let root = arguments.next().ok_or_else(|| {
                    anyhow::anyhow!("--allow-write-root에는 폴더 경로가 필요합니다.")
                })?;
                let root = PathBuf::from(root);
                if !root.is_dir() {
                    anyhow::bail!("--allow-write-root는 존재하는 폴더여야 합니다.");
                }
                roots.push(root);
            }
            Some("--figma-client-id") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--figma-client-id에는 값이 필요합니다."))?;
                let value = value
                    .to_str()
                    .ok_or_else(|| {
                        anyhow::anyhow!("--figma-client-id는 UTF-8 문자열이어야 합니다.")
                    })?
                    .to_owned();
                if value.is_empty() {
                    anyhow::bail!("--figma-client-id는 빈 문자열일 수 없습니다.");
                }
                figma_client_id = Some(value);
            }
            Some("--figma-client-secret") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--figma-client-secret에는 값이 필요합니다."))?;
                let value = value
                    .to_str()
                    .ok_or_else(|| {
                        anyhow::anyhow!("--figma-client-secret는 UTF-8 문자열이어야 합니다.")
                    })?
                    .to_owned();
                if value.is_empty() {
                    anyhow::bail!("--figma-client-secret는 빈 문자열일 수 없습니다.");
                }
                figma_client_secret = Some(value);
            }
            Some("--figma-callback-port") => {
                let value = arguments.next().ok_or_else(|| {
                    anyhow::anyhow!("--figma-callback-port에는 포트 번호가 필요합니다.")
                })?;
                let value = value.to_str().ok_or_else(|| {
                    anyhow::anyhow!("--figma-callback-port는 UTF-8 문자열이어야 합니다.")
                })?;
                figma_callback_port = Some(value.parse::<u16>().map_err(|_| {
                    anyhow::anyhow!("--figma-callback-port는 1-65535 사이 숫자여야 합니다.")
                })?);
            }
            Some(flag) => anyhow::bail!("지원하지 않는 devup-mcp 인자입니다: {flag}"),
            None => anyhow::bail!("devup-mcp 인자는 UTF-8 flag여야 합니다."),
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
    }))
}

pub fn self_check() -> SelfCheckReport {
    let credential_ok = devup_mcp_figma::KeyringCredentialStore::probe().is_ok();
    let (env_client_id, env_client_secret) = env_figma_client_credentials();
    let figma_direct =
        resolve_figma_direct_config(None, None, None, env_client_id, env_client_secret);
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

    let (env_client_id, env_client_secret) = env_figma_client_credentials();
    let figma_direct = resolve_figma_direct_config(
        config.figma_client_id.clone(),
        config.figma_client_secret.clone(),
        config.figma_callback_port,
        env_client_id,
        env_client_secret,
    );
    let service =
        server::DevupServer::production_with_config(config.allowed_write_roots, figma_direct)?
            .serve((tokio::io::stdin(), tokio::io::stdout()))
            .await?;
    service.waiting().await?;
    Ok(())
}
