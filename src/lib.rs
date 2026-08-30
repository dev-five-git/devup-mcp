pub mod codegen;
pub mod figma;
pub mod server;

pub async fn run_stdio() -> anyhow::Result<()> {
    use rmcp::ServiceExt;

    let service = server::DevupServer::default()
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await?;
    service.waiting().await?;
    Ok(())
}
