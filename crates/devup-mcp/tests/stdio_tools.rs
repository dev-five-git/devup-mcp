use devup_mcp::server::DevupServer;
use rmcp::ServiceExt;

#[tokio::test]
async fn exposes_only_the_four_devup_figma_tools() -> anyhow::Result<()> {
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
    let tools = client.list_all_tools().await?;
    let mut names = tools
        .iter()
        .map(|tool| tool.name.to_string())
        .collect::<Vec<_>>();
    names.sort();

    assert_eq!(
        names,
        [
            "devup_figma_auth",
            "devup_figma_continue",
            "devup_figma_to_json",
            "devup_figma_to_ui",
        ]
    );

    let ui = tools
        .iter()
        .find(|tool| tool.name == "devup_figma_to_ui")
        .unwrap();
    let ui_schema = serde_json::to_value(&ui.input_schema)?;
    assert!(ui_schema.to_string().contains("sourcePolicy"));
    assert!(ui_schema.to_string().contains("scope"));
    assert!(!ui_schema.to_string().contains("code"));

    let continuation = tools
        .iter()
        .find(|tool| tool.name == "devup_figma_continue")
        .unwrap();
    let continuation_schema = serde_json::to_value(&continuation.input_schema)?;
    let continuation_text = continuation_schema.to_string();
    assert!(continuation_text.contains("sessionId"));
    assert!(continuation_text.contains("callId"));
    assert!(continuation_text.contains("result"));
    assert!(!continuation_text.contains("code"));

    client.cancel().await?;
    server.await??;
    Ok(())
}
