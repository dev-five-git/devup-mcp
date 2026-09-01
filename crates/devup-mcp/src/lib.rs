pub mod server;

use std::{ffi::OsString, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub allowed_write_roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Version,
    Serve(ServerConfig),
}

pub fn parse_cli_args<I, T>(arguments: I) -> anyhow::Result<CliAction>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut arguments = arguments.into_iter().map(Into::into).peekable();
    let mut roots = Vec::new();
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--version" | "-V") if roots.is_empty() && arguments.peek().is_none() => {
                return Ok(CliAction::Version);
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
            Some(flag) => anyhow::bail!("지원하지 않는 devup-mcp 인자입니다: {flag}"),
            None => anyhow::bail!("devup-mcp 인자는 UTF-8 flag여야 합니다."),
        }
    }
    if roots.is_empty() {
        roots.push(std::env::current_dir()?);
    }
    Ok(CliAction::Serve(ServerConfig {
        allowed_write_roots: roots,
    }))
}

pub async fn run_stdio() -> anyhow::Result<()> {
    let CliAction::Serve(config) = parse_cli_args(std::iter::empty::<OsString>())? else {
        unreachable!("empty arguments always start the server")
    };
    run_stdio_with_config(config).await
}

pub async fn run_stdio_with_config(config: ServerConfig) -> anyhow::Result<()> {
    use rmcp::ServiceExt;

    let service = server::DevupServer::production_with_output_roots(config.allowed_write_roots)?
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await?;
    service.waiting().await?;
    Ok(())
}
