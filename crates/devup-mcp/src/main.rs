#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments
        .next()
        .is_some_and(|argument| matches!(argument.to_str(), Some("--version" | "-V")))
        && arguments.next().is_none()
    {
        println!("devup-mcp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "devup_mcp=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    devup_mcp::run_stdio().await
}
