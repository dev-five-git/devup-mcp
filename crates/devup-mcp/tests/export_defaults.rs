//! What `devup_figma_export` gives you when you ask for nothing in
//! particular, and what its description tells you before you ask.
//!
//! Both Figma tickets in the September usage report received a `devupJson`
//! they did not use and then called `devup_project_context` for the tokens
//! they actually had to match - which is the right order, because a
//! project that already has a `devup.json` must match that file rather
//! than the theme read out of Figma. The default was enlarging every
//! response to answer a question nobody was asking.

mod common;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{AuthStatus, DevupError, FigmaUpstream, ReadToolCall, UpstreamResult};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Map, Value, json};

struct Auth;

#[async_trait]
impl DevupAuth for Auth {
    async fn status(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Connected)
    }
    async fn login(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Connected)
    }
    async fn logout(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Disconnected)
    }
}

#[derive(Default)]
struct Fixture {
    calls: AtomicUsize,
}

#[async_trait]
impl FigmaUpstream for Fixture {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["get_metadata".to_owned(), "use_figma".to_owned()])
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(UpstreamResult {
            raw: match call.tool_name() {
                "get_metadata" => json!({
                    "structuredContent": { "devupMetadata": {
                        "fileKey": "FileKey123", "version": "v1", "rootId": "1:2",
                        "nodes": [{
                            "id": "1:2", "type": "FRAME", "name": "Synthetic Frame",
                            "childrenIds": [], "descendantCount": 1
                        }]
                    }}
                }),
                _ => json!({
                    "fileKey": "FileKey123", "version": "v1", "rootIds": ["1:2"],
                    "nodes": [{
                        "id": "1:2", "type": "FRAME",
                        "fields": {
                            "name": "Synthetic Frame", "childrenIds": [],
                            "layoutMode": "VERTICAL",
                            "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                            "width": 320, "height": 240
                        },
                        "extra": {}, "fieldErrors": {}
                    }]
                }),
            },
        })
    }
}

async fn export(arguments: Value) -> anyhow::Result<Value> {
    let server = DevupServer::new(Services::new(Arc::new(Auth), Arc::new(Fixture::default())));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new("devup_figma_export").with_arguments(arguments))
        .await?;
    client.cancel().await?;
    task.await??;
    anyhow::ensure!(
        result.is_error != Some(true),
        "{}",
        result
            .structured_content
            .as_ref()
            .expect("structured error")
    );
    Ok(result.structured_content.unwrap())
}

async fn export_tool_schema() -> anyhow::Result<(Value, String)> {
    let server = DevupServer::new(Services::new(Arc::new(Auth), Arc::new(Fixture::default())));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let tools = client.list_all_tools().await?;
    let export = tools
        .iter()
        .find(|tool| tool.name == "devup_figma_export")
        .expect("devup_figma_export is published");
    let schema = serde_json::to_value(export.input_schema.as_ref())?;
    let description = export.description.clone().unwrap_or_default().to_string();
    client.cancel().await?;
    task.await??;
    Ok((schema, description))
}

/// Asking for nothing gets the deliverable and only the deliverable.
#[tokio::test]
async fn the_default_output_is_tsx_alone() -> anyhow::Result<()> {
    let output = export(json!({
        "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2"
    }))
    .await?;

    assert!(
        output["tsx"].as_str().is_some_and(|tsx| !tsx.is_empty()),
        "tsx is the deliverable and must still arrive: {output}"
    );
    assert!(
        output.get("devupJson").is_none(),
        "devupJson must not be produced unless it was asked for: {output}"
    );
    // The keys devupJson brings with it must be gone too, not merely empty.
    for key in [
        "themeCounts",
        "themeCompleteness",
        "conflicts",
        "unresolvedVariables",
    ] {
        assert!(
            output.get(key).is_none(),
            "{key} rides along with devupJson and must be absent: {output}"
        );
    }
    Ok(())
}

/// Asking for it explicitly still works - the default moved, the output
/// did not go away.
#[tokio::test]
async fn devup_json_is_still_available_when_asked_for() -> anyhow::Result<()> {
    let output = export(json!({
        "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-2",
        "outputs": ["tsx", "devupJson"]
    }))
    .await?;

    assert!(output["tsx"].as_str().is_some());
    assert!(
        output["devupJson"].as_str().is_some(),
        "an explicit request must still be honoured: {output}"
    );
    Ok(())
}

/// A client chooses what to send from the published schema, so the default
/// has to be right there and not only in the server's behaviour.
#[tokio::test]
async fn the_published_schema_defaults_to_tsx_alone() -> anyhow::Result<()> {
    let (schema, _) = export_tool_schema().await?;
    assert_eq!(
        schema["properties"]["outputs"]["default"],
        json!(["tsx"]),
        "the schema clients read must carry the same default: {}",
        schema["properties"]["outputs"]
    );
    Ok(())
}

/// The description is read before the call is composed, so the three
/// things the usage report asked for have to be in it: when devupJson is
/// worth adding, that known ids go straight to frameIds, and how an asset
/// actually reaches disk.
#[tokio::test]
async fn the_description_carries_the_guidance_the_usage_report_asked_for() -> anyhow::Result<()> {
    let (_, description) = export_tool_schema().await?;

    assert!(
        description.contains("devup_project_context"),
        "it must say what to read instead of devupJson when the project has a devup.json: {description}"
    );
    assert!(
        description.contains("frameIds") && description.contains("devup_figma_explore"),
        "it must say that known ids go straight to frameIds and skip explore: {description}"
    );
    assert!(
        description.contains("assetRequests") && description.contains("outputPath"),
        "it must name the call that actually writes an asset: {description}"
    );
    // W3 settled the recovery for this: the second call uses the original
    // url, because an artifact collected without asset capture cannot serve
    // asset requests - and `artifactId` with `refresh` is itself rejected.
    assert!(
        description.contains("rather than `artifactId`"),
        "it must say the asset call uses url, not artifactId: {description}"
    );
    assert!(
        description.contains("placeholder"),
        "it must say the manifest path is not the file you wrote: {description}"
    );
    Ok(())
}

#[tokio::test]
async fn p3_large_export_is_rejected_before_collection() {
    let result = export(
        json!({"url":"https://www.figma.com/design/FileKey123/Test?node-id=1-2",
        "frameIds": (1..=14).map(|i| format!("1:{i}")).collect::<Vec<_>>(),
        "outputs":["tsx","devupJson","assetManifest"]}),
    )
    .await;
    let message = result.unwrap_err().to_string();
    assert!(message.contains("batch"), "{message}");
}

#[tokio::test]
async fn p3_public_root_requires_absolute_existing_directory() {
    let result = export(
        json!({"url":"https://www.figma.com/design/FileKey123/Test?node-id=1-2",
        "assetPublicRoot":"relative/public", "outputs":["tsx","assetManifest"]}),
    )
    .await;
    assert!(
        result.is_err(),
        "relative assetPublicRoot was silently ignored"
    );
}

#[tokio::test]
async fn p3_parameter_failures_do_not_spend_upstream_calls() -> anyhow::Result<()> {
    let fixture = Arc::new(Fixture::default());
    let server = DevupServer::new(Services::new(Arc::new(Auth), fixture.clone()));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    for arguments in [
        json!({"url":"https://www.figma.com/design/FileKey123/Test?node-id=1-2", "assetPublicRoot":std::env::temp_dir()}),
        json!({"url":"https://www.figma.com/design/FileKey123/Test?node-id=1-2", "outputs":["referencePng"], "frameIds":["1:2"]}),
        json!({"url":"https://www.figma.com/design/FileKey123/Test?node-id=1-2", "assetRequests":[{"assetId":"x"}]}),
        json!({"url":"https://www.figma.com/design/FileKey123/Test?node-id=1-2", "outputPaths":{"tsx":"../escape.tsx"}}),
        json!({"url":"https://www.figma.com/design/FileKey123/Test?node-id=1-2", "frameIds":(1..=14).map(|i| format!("1:{i}")).collect::<Vec<_>>()}),
    ] {
        let error = client
            .call_tool(
                CallToolRequestParams::new("devup_figma_export")
                    .with_arguments(arguments.as_object().unwrap().clone()),
            )
            .await
            .map(common::tool_error)
            .expect("structured failure");
        assert!(error.contains("-32602"), "{error}");
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
    }
    let error = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_export")
                .with_arguments(json!({"artifactId":"missing"}).as_object().unwrap().clone()),
        )
        .await
        .map(common::tool_error)
        .expect("structured failure");
    assert!(
        error.contains("-32603") && error.contains("The Figma artifact is missing or expired."),
        "{error}"
    );
    client.cancel().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
async fn p3_schema_exposes_opt_in_and_cost_and_section_contract() -> anyhow::Result<()> {
    let (schema, description) = export_tool_schema().await?;
    assert!(schema["properties"].get("assetPublicRoot").is_some());
    assert_eq!(schema["properties"]["assetNamesPerNode"]["default"], true);
    for hint in [
        "1–3",
        "6 frames",
        "12 frame-times-output",
        "15–60 seconds",
        "300 seconds",
        "two stages",
        "nextAction.example",
    ] {
        assert!(description.contains(hint), "Missing {hint}");
    }
    Ok(())
}

#[tokio::test]
async fn r6_export_response_identifies_responding_build() -> anyhow::Result<()> {
    let output =
        export(json!({"url":"https://www.figma.com/design/FileKey123/Fixture?node-id=1-2"}))
            .await?;
    assert_eq!(output["server"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(output["server"]["buildId"], devup_mcp::build_id());
    assert_eq!(
        output["server"]["commit"],
        option_env!("DEVUP_MCP_GIT_COMMIT")
            .filter(|s| !s.is_empty())
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null)
    );
    Ok(())
}
#[tokio::test]
async fn r6_source_map_description_explains_cached_reprojection() -> anyhow::Result<()> {
    let (_, description) = export_tool_schema().await?;
    assert!(description.contains("same call") && description.contains(r#"{"artifactId":"<artifactId>","outputs":["tsx","rawSnapshot","sourceMap"],"debug":true}"#),"{description}");
    Ok(())
}
