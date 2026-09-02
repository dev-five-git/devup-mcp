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

    devup_mcp::run_stdio_with_config(config).await
}
