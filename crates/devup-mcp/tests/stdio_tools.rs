use devup_mcp::server::DevupServer;
use rmcp::ServiceExt;

#[tokio::test]
async fn exposes_only_the_three_devup_figma_tools() -> anyhow::Result<()> {
    let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
    let server = tokio::spawn(async move {
        DevupServer::default()
            .serve(server_transport)
            .await?
            .waiting()
            .await?;
        anyhow::Ok(())
    });

    let client = ().serve(client_transport).await?;
    let mut names = client
        .list_all_tools()
        .await?
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect::<Vec<_>>();
    names.sort();

    assert_eq!(
        names,
        [
            "devup_figma_auth",
            "devup_figma_to_json",
            "devup_figma_to_ui",
        ]
    );

    client.cancel().await?;
    server.await??;
    Ok(())
}
