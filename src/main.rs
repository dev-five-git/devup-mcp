#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "devup_mcp=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    devup_mcp::run_stdio().await
}
