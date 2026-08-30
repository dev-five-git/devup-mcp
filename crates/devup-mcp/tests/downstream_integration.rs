use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{AuthStatus, DevupError, FigmaUpstream, ReadToolCall, UpstreamResult};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Map, Value, json};

#[derive(Debug)]
struct ConnectedAuth;

#[async_trait]
impl DevupAuth for ConnectedAuth {
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

#[derive(Debug)]
struct FixtureUpstream;

#[async_trait]
impl FigmaUpstream for FixtureUpstream {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        let payload = match call {
            ReadToolCall::Metadata { .. } => {
                json!({
                    "devupMetadata": {
                        "fileKey": "85CgSws3o5XsLv7aAwWJyS",
                        "version": "1",
                        "rootId": "3879:35481",
                        "nodes": [{
                            "id": "3879:35481",
                            "type": "FRAME",
                            "childrenIds": [],
                            "descendantCount": 1
                        }]
                    }
                })
            }
            ReadToolCall::Snapshot {
                script: devup_mcp_figma::BuiltinScript::NodeSnapshot,
                ..
            } => {
                json!({
                    "fileKey": "85CgSws3o5XsLv7aAwWJyS", "version": "1", "rootIds": ["3879:35481"],
                    "nodes": [{
                        "id": "3879:35481", "type": "FRAME",
                        "fields": {"name": "Proofread", "childrenIds": [], "layoutMode": "VERTICAL", "width": 320, "height": 240},
                        "extra": {"futureField": true}, "fieldErrors": {}
                    }], "diagnostics": []
                })
            }
            ReadToolCall::Snapshot { .. } => json!({
                "collections": [{"id": "c", "name": "Theme", "defaultModeId": "m", "modes": [{"modeId": "m", "name": "Default"}]}],
                "variables": [{
                    "id": "v", "name": "Color/Primary", "resolvedType": "COLOR", "variableCollectionId": "c",
                    "codeSyntax": {"WEB": "primary"}, "valuesByMode": {"m": {"r": 0, "g": 0.4, "b": 1, "a": 1}}
                }],
                "styles": [], "usedRemoteVariables": [], "localComplete": true, "usedRemoteComplete": false
            }),
            _ => {
                return Err(devup_mcp_figma::DevupError::new(
                    devup_mcp_figma::ErrorCode::DevupSnapshotUnsupported,
                    "unexpected test call",
                    false,
                ));
            }
        };
        Ok(UpstreamResult {
            raw: json!({"structuredContent": {"result": payload}}),
        })
    }
}

async fn call_tool(name: &str, arguments: Value) -> anyhow::Result<Value> {
    call_tool_with_auth(Arc::new(ConnectedAuth), name, arguments).await
}

async fn call_tool_with_auth(
    auth: Arc<dyn DevupAuth>,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    let server = DevupServer::new(Services::new(auth, Arc::new(FixtureUpstream)));
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(arguments))
        .await?;
    client.cancel().await?;
    task.await??;
    Ok(result.structured_content.expect("structured tool output"))
}

#[derive(Debug, Default)]
struct LoginAuth {
    logins: AtomicUsize,
}

#[async_trait]
impl DevupAuth for LoginAuth {
    async fn status(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Disconnected)
    }

    async fn login(&self) -> Result<AuthStatus, DevupError> {
        self.logins.fetch_add(1, Ordering::SeqCst);
        Ok(AuthStatus::Connected)
    }

    async fn logout(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Disconnected)
    }
}

#[tokio::test]
async fn conversion_returns_host_handoff_without_starting_oauth() -> anyhow::Result<()> {
    let auth = Arc::new(LoginAuth::default());
    let output = call_tool_with_auth(
        auth.clone(),
        "devup_figma_to_ui",
        json!({"url": "https://figma.com/design/85CgSws3o5XsLv7aAwWJyS/Name?node-id=3879-35481"}),
    )
    .await?;

    assert_eq!(output["status"], "needs_figma");
    assert_eq!(auth.logins.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn converts_a_figma_link_to_structured_devup_ui() -> anyhow::Result<()> {
    let result = call_tool(
        "devup_figma_to_ui",
        json!({
            "url": "https://www.figma.com/design/85CgSws3o5XsLv7aAwWJyS/Name?node-id=3879-35481",
            "includeDiagnostics": true
        }),
    )
    .await?;

    assert_eq!(result["status"], "complete");
    assert!(
        result["tsx"]
            .as_str()
            .unwrap()
            .contains("export function Proofread")
    );
    assert_eq!(result["source"]["nodeId"], "3879:35481");
    assert_eq!(result["snapshot"]["preservedNodeCount"], 1);
    Ok(())
}

#[tokio::test]
async fn converts_figma_variables_to_structured_devup_json() -> anyhow::Result<()> {
    let result = call_tool(
        "devup_figma_to_json",
        json!({
            "url": "https://www.figma.com/design/85CgSws3o5XsLv7aAwWJyS/Name?node-id=3879-35481",
            "scope": "file",
            "includeDiagnostics": true
        }),
    )
    .await?;

    assert_eq!(result["status"], "complete");
    assert!(
        result["devupJson"]
            .as_str()
            .unwrap()
            .contains("\"primary\"")
    );
    assert_eq!(result["completeness"], "used-tokens");
    Ok(())
}
