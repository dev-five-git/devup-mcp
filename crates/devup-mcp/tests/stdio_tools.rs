use devup_mcp::server::DevupServer;
use rmcp::ServiceExt;

#[tokio::test]
async fn exposes_the_seven_read_only_devup_figma_tools() -> anyhow::Result<()> {
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
    let server_info = client.peer_info().expect("server info");
    let resources = server_info
        .capabilities
        .resources
        .as_ref()
        .expect("resources capability");
    assert_eq!(resources.subscribe, None);
    assert_eq!(resources.list_changed, None);
    assert_eq!(client.list_all_resources().await?, Vec::new());
    assert_eq!(client.list_all_resource_templates().await?.len(), 2);
    let tools = client.list_all_tools().await?;
    assert!(
        tools.iter().all(|tool| tool.output_schema.is_some()),
        "native resource-link responses must preserve the previous structured output schemas"
    );
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
            "devup_figma_explore",
            "devup_figma_export",
            "devup_figma_search",
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
    assert!(ui_schema.to_string().contains("rootLayout"));
    assert!(!ui_schema.to_string().contains("code"));

    let export = tools
        .iter()
        .find(|tool| tool.name == "devup_figma_export")
        .unwrap();
    let export_schema = serde_json::to_value(&export.input_schema)?;
    let export_text = export_schema.to_string();
    for field in [
        "url",
        "artifactId",
        "outputs",
        "scope",
        "rootLayout",
        "strict",
        "refresh",
        "outputPaths",
        "frameIds",
        "allScreens",
        "delivery",
        "sourcePolicy",
    ] {
        assert!(export_text.contains(field), "missing export field {field}");
    }
    assert!(!export_text.contains("accessToken"));
    assert!(!export_text.contains("clientSecret"));

    let explore = tools
        .iter()
        .find(|tool| tool.name == "devup_figma_explore")
        .unwrap();
    let explore_schema = serde_json::to_value(&explore.input_schema)?;
    let explore_text = explore_schema.to_string();
    assert!(explore_text.contains("url"));
    assert!(explore_text.contains("limit"));
    assert!(explore_text.contains("includeTextPreview"));
    assert!(explore_text.contains("sourcePolicy"));
    assert!(!explore_text.contains("code"));

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
