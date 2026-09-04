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
                        "fields": {
                            "name": "Proofread",
                            "childrenIds": [],
                            "layoutMode": "VERTICAL",
                            "width": 320,
                            "height": 240,
                            "fills": [{
                                "type": "SOLID",
                                "color": {"r": 0, "g": 0.4, "b": 1, "a": 1},
                                "boundVariables": {
                                    "color": {"type": "VARIABLE_ALIAS", "id": "v"}
                                }
                            }],
                            "boundVariables": {
                                "fills": [{"type": "VARIABLE_ALIAS", "id": "v"}]
                            }
                        },
                        "extra": {"futureField": true}, "fieldErrors": {}
                    }], "diagnostics": []
                })
            }
            ReadToolCall::Snapshot {
                script: devup_mcp_figma::BuiltinScript::VariableCatalog,
                ..
            } => json!({
                "collections": [{
                    "id": "c", "name": "Theme", "defaultModeId": "m",
                    "modes": [{"modeId": "m", "name": "Default"}]
                }],
                "variableIds": ["v", "unused", "v-alt"], "styles": [],
                "localComplete": true, "usedRemoteComplete": false
            }),
            ReadToolCall::Snapshot {
                script: devup_mcp_figma::BuiltinScript::LocalVariables,
                ..
            } => json!({
                "variables": [{
                    "id": "v", "name": "Color/Primary", "resolvedType": "COLOR", "variableCollectionId": "c",
                    "codeSyntax": {"WEB": "primary"}, "valuesByMode": {"m": {"r": 0, "g": 0.4, "b": 1, "a": 1}}
                }, {
                    "id": "unused", "name": "Color/Unused", "resolvedType": "COLOR", "variableCollectionId": "c",
                    "codeSyntax": {"WEB": "unused"}, "valuesByMode": {"m": {"r": 1, "g": 0, "b": 0, "a": 1}}
                }, {
                    "id": "v-alt", "name": "Color/PrimaryAlt", "resolvedType": "COLOR", "variableCollectionId": "c",
                    "codeSyntax": {"WEB": "primary"}, "valuesByMode": {"m": {"r": 1, "g": 0, "b": 0, "a": 1}}
                }],
                "styles": []
            }),
            ReadToolCall::Snapshot {
                script: devup_mcp_figma::BuiltinScript::UsedResources,
                ..
            } => json!({
                "collections": [{
                    "id": "c", "name": "Theme", "defaultModeId": "m",
                    "modes": [{"modeId": "m", "name": "Default"}]
                }],
                "variables": [{
                    "id": "v", "name": "Color/Primary", "resolvedType": "COLOR", "variableCollectionId": "c",
                    "codeSyntax": {"WEB": "primary"}, "valuesByMode": {"m": {"r": 0, "g": 0.4, "b": 1, "a": 1}}
                }],
                "styles": [],
                "unresolved": []
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
    call_tool_with_services(auth, Arc::new(FixtureUpstream), name, arguments).await
}

async fn call_tool_with_services(
    auth: Arc<dyn DevupAuth>,
    upstream: Arc<dyn FigmaUpstream>,
    name: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    let server = DevupServer::new(Services::new(auth, upstream));
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

async fn call_tool_with_output_roots(
    name: &str,
    arguments: Value,
    roots: Vec<std::path::PathBuf>,
) -> anyhow::Result<Value> {
    let server = DevupServer::with_output_roots(
        Services::new(Arc::new(ConnectedAuth), Arc::new(FixtureUpstream)),
        roots,
    )?;
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

#[derive(Debug)]
struct PartialFixtureUpstream;

#[async_trait]
impl FigmaUpstream for PartialFixtureUpstream {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        if let ReadToolCall::Snapshot {
            script: devup_mcp_figma::BuiltinScript::NodeSnapshot,
            ..
        } = call
        {
            return Ok(UpstreamResult {
                raw: json!({"structuredContent": {"result": {
                    "fileKey": "85CgSws3o5XsLv7aAwWJyS",
                    "version": "1",
                    "rootIds": ["3879:35481"],
                    "nodes": [{
                        "id": "3879:35481",
                        "type": "FRAME",
                        "fields": {
                            "name": "Proofread",
                            "childrenIds": ["3879:404"],
                            "layoutMode": "VERTICAL",
                            "width": 320,
                            "height": 240
                        },
                        "extra": {},
                        "fieldErrors": {}
                    }],
                    "diagnostics": []
                }}}),
            });
        }
        FixtureUpstream.call_read_tool(call).await
    }
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

/// Converting says what it needs rather than reaching for the browser on its
/// own. A tool that logs a user in as a side effect of asking for code decides
/// something they did not ask it to decide, and the request that provoked it is
/// gone by the time they see the window.
#[tokio::test]
async fn conversion_asks_to_be_logged_in_rather_than_starting_oauth() -> anyhow::Result<()> {
    let auth = Arc::new(LoginAuth::default());
    let error = call_tool_with_auth(
        auth.clone(),
        "devup_figma_to_ui",
        json!({"url": "https://figma.com/design/85CgSws3o5XsLv7aAwWJyS/Name?node-id=3879-35481"}),
    )
    .await
    .expect_err("a disconnected direct path cannot convert");

    assert!(
        error.to_string().contains("devup_figma_auth login"),
        "the error should name the action that fixes it: {error}"
    );
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
    assert_eq!(result["quality"]["acquisition"], "complete");
    assert_eq!(result["quality"]["projection"], "exact");
    assert_eq!(result["quality"]["theme"], "not-requested");
    assert_eq!(result["quality"]["assets"], "not-requested");
    assert!(
        result["tsx"]
            .as_str()
            .unwrap()
            .contains("export function Proofread")
    );
    let tsx = result["tsx"].as_str().unwrap();
    assert!(tsx.contains("bg=\"$primary\""));
    assert!(!tsx.contains("$colorPrimary"));
    assert_eq!(result["source"]["nodeId"], "3879:35481");
    assert_eq!(result["snapshot"]["preservedNodeCount"], 1);
    Ok(())
}

#[tokio::test]
async fn reports_partial_instead_of_complete_when_a_child_is_missing() -> anyhow::Result<()> {
    let result = call_tool_with_services(
        Arc::new(ConnectedAuth),
        Arc::new(PartialFixtureUpstream),
        "devup_figma_to_ui",
        json!({
            "url": "https://www.figma.com/design/85CgSws3o5XsLv7aAwWJyS/Name?node-id=3879-35481",
            "includeDiagnostics": true
        }),
    )
    .await?;

    assert_eq!(result["status"], "partial");
    assert_eq!(result["completenessReport"]["state"], "partial");
    assert_eq!(
        result["completenessReport"]["snapshot"]["missingChildren"][0]["childId"],
        "3879:404"
    );
    Ok(())
}

#[tokio::test]
async fn converts_figma_variables_to_structured_devup_json() -> anyhow::Result<()> {
    let output_root = std::env::temp_dir().join(format!(
        "devup-mcp-output-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(&output_root)?;
    let output_path = output_root.join("devup.json");
    let result = call_tool_with_output_roots(
        "devup_figma_to_json",
        json!({
            "url": "https://www.figma.com/design/85CgSws3o5XsLv7aAwWJyS/Name?node-id=3879-35481",
            "scope": "file",
            "includeDiagnostics": true,
            "outputPath": output_path
        }),
        vec![output_root.clone()],
    )
    .await?;

    assert_eq!(result["status"], "partial");
    assert_eq!(result["quality"]["acquisition"], "complete");
    assert_eq!(result["quality"]["projection"], "not-requested");
    assert_eq!(result["quality"]["theme"], "conflicted");
    assert!(
        result["devupJson"]
            .as_str()
            .unwrap()
            .contains("\"primary\"")
    );
    assert_eq!(result["completeness"], "used-tokens");
    assert_eq!(result["conflicts"][0]["token"], "primary");
    assert_eq!(result["conflicts"][0]["winnerVariableId"], "v");
    assert_eq!(result["unresolvedVariables"], json!([]));
    assert_eq!(
        std::fs::read_to_string(&output_path)?,
        result["devupJson"].as_str().unwrap()
    );
    assert!(result["outputPath"].as_str().is_some());
    std::fs::remove_file(output_path)?;
    std::fs::remove_dir(output_root)?;
    Ok(())
}

#[tokio::test]
async fn node_theme_scope_excludes_file_variables_not_used_by_the_node() -> anyhow::Result<()> {
    let result = call_tool(
        "devup_figma_to_json",
        json!({
            "url": "https://www.figma.com/design/85CgSws3o5XsLv7aAwWJyS/Name?node-id=3879-35481",
            "scope": "node",
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
    assert!(!result["devupJson"].as_str().unwrap().contains("\"unused\""));
    assert_eq!(result["completeness"], "used-tokens");
    Ok(())
}
