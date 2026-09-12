use std::sync::Arc;

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

fn args(root: &std::path::Path) -> Value {
    json!({"url":"https://www.figma.com/design/85CgSws3o5XsLv7aAwWJyS/Test?node-id=3879-35481",
        "outputs":["pageScaffold"], "componentName":"Proofread", "pageScaffold":{"route":"proofread"},
        "outputPaths":{"pageScaffold":root}, "delivery":"inline"})
}
async fn export(root: &std::path::Path, args: Value) -> Value {
    call_tool_with_output_roots("devup_figma_export", args, vec![root.to_path_buf()])
        .await
        .unwrap()
}
fn files(value: &Value) -> &Vec<Value> {
    value["pageScaffold"]["files"]
        .as_array()
        .unwrap_or_else(|| panic!("missing scaffold: {value}"))
}
#[tokio::test]
async fn w7_preview_is_default_and_writes_nothing() {
    let dir = temp_dir().unwrap();
    let value = export(dir.path(), args(dir.path())).await;
    assert_eq!(files(&value).len(), 2);
    assert_eq!(value["pageScaffold"]["mode"], "preview");
    assert_eq!(
        value["outputPathResults"]["pathKinds"]["pageScaffold"],
        "directory"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    assert!(value.get("tsx").is_none());
    assert!(value.get("assetManifest").is_none());
}
#[tokio::test]
async fn w7_explicit_write_matches_manifest_exactly() {
    let dir = temp_dir().unwrap();
    let mut input = args(dir.path());
    input["pageScaffold"]["write"] = json!(true);
    let value = export(dir.path(), input).await;
    assert_eq!(files(&value).len(), 2);
    for file in files(&value) {
        assert_eq!(
            std::fs::read_to_string(file["path"].as_str().unwrap()).unwrap(),
            file["content"]
        );
    }
    assert_eq!(
        written_files(dir.path()),
        std::collections::BTreeSet::from([
            dir.path().join("src/app/proofread/page.tsx"),
            dir.path().join("components/pages/proofread/Proofread.tsx"),
        ])
    );
    assert_eq!(value["pageScaffold"]["collisionPolicy"], "refuse-existing");
}
#[tokio::test]
async fn w7_existing_file_refusal_names_collision() {
    let dir = temp_dir().unwrap();
    let page = dir.path().join("src/app/proofread/page.tsx");
    std::fs::create_dir_all(page.parent().unwrap()).unwrap();
    std::fs::write(&page, "original").unwrap();
    let mut input = args(dir.path());
    input["pageScaffold"]["write"] = json!(true);
    let value = export(dir.path(), input).await;
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("page.tsx"),
        "{value}"
    );
    assert_eq!(std::fs::read_to_string(page).unwrap(), "original");
    assert!(
        !dir.path()
            .join("components/pages/proofread/Proofread.tsx")
            .exists()
    );
}
#[tokio::test]
async fn w7_out_of_root_refused_before_writes() {
    let dir = temp_dir().unwrap();
    let outside = temp_dir().unwrap();
    let mut input = args(outside.path());
    input["pageScaffold"]["write"] = json!(true);
    let value = export(dir.path(), input).await;
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("outside"),
        "{value}"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}
#[tokio::test]
async fn w7_page_shell_and_named_screen_have_no_inferred_client_boundary() {
    let dir = temp_dir().unwrap();
    let value = export(dir.path(), args(dir.path())).await;
    let files = files(&value);
    let page = files
        .iter()
        .find(|f| f["path"].as_str().unwrap().ends_with("page.tsx"))
        .unwrap();
    assert!(
        page["content"]
            .as_str()
            .unwrap()
            .contains("export default function ProofreadPage()")
    );
    let screen = files
        .iter()
        .find(|f| f["path"].as_str().unwrap().ends_with("Proofread.tsx"))
        .unwrap();
    assert!(!screen["path"].as_str().unwrap().contains("src/app"));
    assert!(
        screen["content"]
            .as_str()
            .unwrap()
            .contains("export function Proofread()")
    );
    for file in files {
        assert!(!file["content"].as_str().unwrap().contains("use client"));
    }
    assert_eq!(
        value["pageScaffold"]["unresolvedDecisions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
#[tokio::test]
async fn w7_route_is_required_and_never_invented() {
    let dir = temp_dir().unwrap();
    let mut input = args(dir.path());
    input.as_object_mut().unwrap().remove("pageScaffold");
    let value = export(dir.path(), input).await;
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("route"),
        "{value}"
    );
}

#[tokio::test]
async fn w7_unsupported_output_key_is_reported_without_writing() {
    let dir = temp_dir().unwrap();
    let mut input = args(dir.path());
    input["outputPaths"]["frame:unknown:pageScaffold"] = json!(dir.path().join("ignored"));
    let value = export(dir.path(), input).await;
    assert_eq!(files(&value).len(), 2);
    assert!(
        value["outputPathResults"]["supportedKeys"]
            .as_array()
            .unwrap()
            .contains(&json!("pageScaffold"))
    );
    assert!(
        value["outputPathResults"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["key"] == "frame:unknown:pageScaffold")
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
#[tokio::test]
async fn w7_traversal_route_is_rejected_without_writing() {
    let dir = temp_dir().unwrap();
    let mut input = args(dir.path());
    input["pageScaffold"]["route"] = json!("../escape");
    input["pageScaffold"]["write"] = json!(true);
    let value = export(dir.path(), input).await;
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("route"),
        "{value}"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

struct TestDir(std::path::PathBuf);
impl TestDir {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn temp_dir() -> std::io::Result<TestDir> {
    let path = std::env::temp_dir().join(format!("devup-w7-{:016x}", rand::random::<u64>()));
    std::fs::create_dir(&path)?;
    Ok(TestDir(dunce::canonicalize(path)?))
}

#[tokio::test]
async fn w7_written_output_keys_are_only_requested_keys() {
    let dir = temp_dir().unwrap();
    let mut input = args(dir.path());
    input["pageScaffold"]["write"] = json!(true);
    let value = export(dir.path(), input).await;
    assert_eq!(
        value["outputPaths"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        vec!["pageScaffold"]
    );
}
#[tokio::test]
async fn w7_resource_delivery_includes_scaffold_manifest() {
    let dir = temp_dir().unwrap();
    let mut input = args(dir.path());
    input["delivery"] = json!("resource");
    let value = export(dir.path(), input).await;
    assert!(value.get("pageScaffold").is_none(), "{value}");
    assert!(
        value["resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["name"] == "page-scaffold.json")
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn w7_explicit_overwrite_preserves_unrelated_files() {
    let dir = temp_dir().unwrap();
    let page = dir.path().join("src/app/proofread/page.tsx");
    std::fs::create_dir_all(page.parent().unwrap()).unwrap();
    std::fs::write(&page, "original").unwrap();
    std::fs::write(dir.path().join("keep.txt"), "untouched").unwrap();
    let mut input = args(dir.path());
    input["pageScaffold"]["write"] = json!(true);
    input["pageScaffold"]["overwrite"] = json!(true);
    let value = export(dir.path(), input).await;
    assert_eq!(files(&value).len(), 2);
    assert!(
        std::fs::read_to_string(page)
            .unwrap()
            .contains("export default function ProofreadPage")
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("keep.txt")).unwrap(),
        "untouched"
    );
    assert_eq!(
        value["pageScaffold"]["collisionPolicy"],
        "replace-with-rollback"
    );
}

fn written_files(root: &std::path::Path) -> std::collections::BTreeSet<std::path::PathBuf> {
    let mut files = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            files.extend(written_files(&entry.path()));
        } else {
            files.insert(entry.path());
        }
    }
    files
}

#[tokio::test]
async fn w7_file_target_is_diagnosed_as_requiring_a_directory() {
    let dir = temp_dir().unwrap();
    let mut input = args(dir.path());
    input["outputPaths"]["pageScaffold"] = json!(dir.path().join("Screen.tsx"));
    input["pageScaffold"]["write"] = json!(true);
    let value = export(dir.path(), input).await;
    assert!(
        value["outputPathResults"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["key"] == "pageScaffold" && d["expectedPathKind"] == "directory"),
        "{value}"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
