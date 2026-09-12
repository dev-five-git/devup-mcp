//! Whether the generated TSX fits the project it is going into.
//!
//! Tokens are named after the Figma variables a screen uses. For a project
//! with no `devup.json` that is the right answer. For one that already has
//! its own names it is not, and the export had no way to know them: a
//! Braillify Studio login screen came back referencing nine tokens that
//! project does not define, `devup_ui_validate` refused the very same code
//! against the very same root, and the export said nothing about it.

use std::{fs, path::PathBuf, sync::Arc};

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

/// One frame filled with a variable the Figma file calls `primary`.
#[derive(Debug)]
struct PrimaryFillUpstream;

#[async_trait]
impl FigmaUpstream for PrimaryFillUpstream {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }
    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        let payload = match call {
            ReadToolCall::Metadata { .. } => json!({"devupMetadata": {
                "fileKey": "85CgSws3o5XsLv7aAwWJyS", "version": "1", "rootId": "3879:35481",
                "nodes": [{"id": "3879:35481", "type": "FRAME", "childrenIds": [], "descendantCount": 1}]
            }}),
            ReadToolCall::Snapshot {
                script: devup_mcp_figma::BuiltinScript::NodeSnapshot,
                ..
            } => json!({
                "fileKey": "85CgSws3o5XsLv7aAwWJyS", "version": "1", "rootIds": ["3879:35481"],
                "nodes": [{
                    "id": "3879:35481", "type": "FRAME",
                    "fields": {
                        "name": "Proofread", "childrenIds": [], "layoutMode": "VERTICAL",
                        "width": 320, "height": 240,
                        "fills": [{
                            "type": "SOLID", "color": {"r": 0, "g": 0.4, "b": 1, "a": 1},
                            "boundVariables": {"color": {"type": "VARIABLE_ALIAS", "id": "v"}}
                        }],
                        "boundVariables": {"fills": [{"type": "VARIABLE_ALIAS", "id": "v"}]}
                    },
                    "extra": {}, "fieldErrors": {}
                }], "diagnostics": []
            }),
            ReadToolCall::Snapshot {
                script: devup_mcp_figma::BuiltinScript::VariableCatalog,
                ..
            } => json!({
                "collections": [{"id": "c", "name": "Theme", "defaultModeId": "m",
                    "modes": [{"modeId": "m", "name": "Default"}]}],
                "variableIds": ["v"], "styles": [],
                "localComplete": true, "usedRemoteComplete": true
            }),
            ReadToolCall::Snapshot {
                script: devup_mcp_figma::BuiltinScript::LocalVariables,
                ..
            } => json!({"variables": [{
                "id": "v", "name": "Color/Primary", "resolvedType": "COLOR",
                "variableCollectionId": "c", "codeSyntax": {"WEB": "primary"},
                "valuesByMode": {"m": {"r": 0, "g": 0.4, "b": 1, "a": 1}}
            }], "styles": []}),
            ReadToolCall::Snapshot {
                script: devup_mcp_figma::BuiltinScript::UsedResources,
                ..
            } => json!({
                "collections": [{"id": "c", "name": "Theme", "defaultModeId": "m",
                    "modes": [{"modeId": "m", "name": "Default"}]}],
                "variables": [{
                    "id": "v", "name": "Color/Primary", "resolvedType": "COLOR",
                    "variableCollectionId": "c", "codeSyntax": {"WEB": "primary"},
                    "valuesByMode": {"m": {"r": 0, "g": 0.4, "b": 1, "a": 1}}
                }],
                "styles": [], "unresolved": []
            }),
            _ => {
                return Err(DevupError::new(
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

async fn export(arguments: Value) -> anyhow::Result<Value> {
    let server = DevupServer::new(Services::new(
        Arc::new(ConnectedAuth),
        Arc::new(PrimaryFillUpstream),
    ));
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
    let structured = result.structured_content.clone();
    client.cancel().await?;
    task.await??;
    anyhow::ensure!(result.is_error != Some(true), "{structured:?}");
    Ok(structured.expect("structured output"))
}

/// A project whose `devup.json` names this colour something else entirely.
fn project_naming_it_brand() -> anyhow::Result<PathBuf> {
    let root = std::env::temp_dir().join(format!("devup-f3-{}", std::process::id()));
    fs::create_dir_all(&root)?;
    fs::write(root.join("package.json"), r#"{"name":"fixture"}"#)?;
    fs::write(
        root.join("devup.json"),
        r##"{"theme":{"colors":{"default":{"brand":"#0066FF"}},"typography":{},"length":{},"shadow":{}}}"##,
    )?;
    Ok(root)
}

const URL: &str = "https://www.figma.com/design/85CgSws3o5XsLv7aAwWJyS/Fixture?node-id=3879-35481";

#[tokio::test]
async fn an_export_without_a_project_root_reads_nothing_from_disk() -> anyhow::Result<()> {
    let result = export(json!({"url": URL, "outputs": ["tsx"]})).await?;
    assert!(
        result["tsx"].as_str().unwrap_or_default().contains("$primary"),
        "the fixture must generate the Figma name: {}",
        result["tsx"]
    );
    assert!(
        result.get("projectThemeValidation").is_none(),
        "no root was given, so nothing may be claimed about any project"
    );
    Ok(())
}

/// The defect, stated as the caller experiences it: the export hands back
/// TSX this server's own validator refuses against the project it is for,
/// and only a separate `devup_ui_validate` call revealed it.
#[tokio::test]
async fn an_export_names_the_tokens_the_project_does_not_define() -> anyhow::Result<()> {
    let root = project_naming_it_brand()?;
    let result = export(json!({
        "url": URL,
        "outputs": ["tsx"],
        "projectRoot": root.to_string_lossy(),
    }))
    .await?;

    let verdict = &result["projectThemeValidation"];
    assert_eq!(verdict["state"], "checked", "{result}");
    assert_eq!(verdict["themeAvailable"], true);
    let tsx = &verdict["outputs"]["tsx"];
    assert_eq!(tsx["ok"], false, "$primary is not in this project: {tsx}");
    assert_eq!(tsx["unknownTokenCount"], 1, "{tsx}");
    assert!(
        tsx["unknownTokens"][0]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("primary"),
        "the verdict names the token: {tsx}"
    );
    assert!(
        verdict["note"]
            .as_str()
            .unwrap_or_default()
            .contains("map each unknown token"),
        "{verdict}"
    );
    Ok(())
}

/// A project that does define the name is told so, rather than left to infer
/// it from an absent warning.
#[tokio::test]
async fn an_export_confirms_a_project_that_already_has_the_token() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("devup-f3-ok-{}", std::process::id()));
    fs::create_dir_all(&root)?;
    fs::write(root.join("package.json"), r#"{"name":"fixture"}"#)?;
    fs::write(
        root.join("devup.json"),
        r##"{"theme":{"colors":{"default":{"primary":"#0066FF"}},"typography":{},"length":{},"shadow":{}}}"##,
    )?;
    let result = export(json!({
        "url": URL,
        "outputs": ["tsx"],
        "projectRoot": root.to_string_lossy(),
    }))
    .await?;
    let tsx = &result["projectThemeValidation"]["outputs"]["tsx"];
    assert_eq!(tsx["unknownTokenCount"], 0, "{tsx}");
    assert_eq!(tsx["ok"], true, "{tsx}");
    Ok(())
}
