#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let action = devup_mcp::parse_cli_args(std::env::args_os().skip(1))?;
    if action == devup_mcp::CliAction::Version {
        println!(
            "devup-mcp {} ({})",
            env!("CARGO_PKG_VERSION"),
            devup_mcp::build_id()
        );
        return Ok(());
    }
    if action == devup_mcp::CliAction::SelfCheck {
        println!("{}", serde_json::to_string(&devup_mcp::self_check())?);
        return Ok(());
    }
    if let devup_mcp::CliAction::MergeAssetBatches(paths) = &action {
        println!(
            "{}",
            serde_json::to_string_pretty(&devup_mcp::asset_batches::merge_files(paths)?)?
        );
        return Ok(());
    }
    if let devup_mcp::CliAction::InstallSkills(config) = &action {
        println!(
            "{}",
            serde_json::to_string_pretty(&devup_mcp::skills::install(config).await?)?
        );
        return Ok(());
    }
    if let devup_mcp::CliAction::CheckSkills(config) = &action {
        println!(
            "{}",
            serde_json::to_string_pretty(&devup_mcp::skills::check(config).await?)?
        );
        return Ok(());
    }
    let devup_mcp::CliAction::Serve(config) = action else {
        unreachable!("version action returned above")
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "devup_mcp=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    // Before the server exists, and therefore before the host has handed it a
    // stdio pipe. A binary swapped after that point could not recover the pipe
    // the host already holds, which is exactly why this runs here and why
    // nothing replaces the running image later.
    if let Some(version) = devup_mcp::server::self_update::promote() {
        // stderr, because stdout carries MCP frames and nothing else.
        eprintln!(
            "devup-mcp: promoted staged release {version}; this process still runs {}",
            env!("CARGO_PKG_VERSION")
        );
    }

    devup_mcp::run_stdio_with_config(config).await
}
