//! `widthBehavior` and the opt-in `widthPolicy` through the real export tool:
//! the default answer is unchanged and says what a wider container would do,
//! and a policy naming a node the capture lacks is refused, not ignored.

use std::sync::Arc;

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

fn node(id: &str, kind: &str, fields: Value) -> Value {
    let mut fields = fields;
    fields["parentId"] = json!("2:0");
    fields["visible"] = json!(true);
    fields["layoutPositioning"] = json!("AUTO");
    fields["constraints"] = json!({"horizontal": "MIN", "vertical": "MIN"});
    let (x, y, w, h) = (
        fields["x"].as_f64().unwrap(),
        fields["y"].as_f64().unwrap(),
        fields["width"].as_f64().unwrap(),
        fields["height"].as_f64().unwrap(),
    );
    fields["absoluteBoundingBox"] = json!({"x": 1000.0 + x, "y": y, "width": w, "height": h});
    json!({"id": id, "type": kind, "fields": fields, "extra": {}, "fieldErrors": {}})
}

fn snapshot() -> Value {
    let fill = json!([{"type": "SOLID", "visible": true, "color": {"r": 1.0, "g": 1.0, "b": 1.0}}]);
    json!({
        "fileKey": "WidthKey", "version": "v1", "rootIds": ["2:0"],
        "nodes": [
            {"id": "2:0", "type": "FRAME", "fields": {
                "name": "Screen", "childrenIds": ["2:4", "2:6", "2:7"], "visible": true,
                "layoutMode": "NONE", "layoutPositioning": "AUTO",
                "layoutSizingHorizontal": "FIXED", "layoutSizingVertical": "FIXED",
                "width": 360.0, "height": 760.0, "fills": fill,
                "absoluteBoundingBox": {"x": 1000.0, "y": 0.0, "width": 360.0, "height": 760.0},
                "parentId": "0:1", "parentType": "SECTION"
            }, "extra": {}, "fieldErrors": {}},
            node("2:4", "RECTANGLE", json!({"name": "card", "childrenIds": [],
                "x": 20.0, "y": 193.0, "width": 320.0, "height": 200.0, "fills": fill})),
            node("2:6", "RECTANGLE", json!({"name": "badge", "childrenIds": [],
                "x": 259.89, "y": 210.0, "width": 64.11, "height": 22.0, "fills": fill})),
            node("2:7", "TEXT", json!({"name": "20 left", "characters": "20 left",
                "styledTextSegments": [{"characters": "20 left"}], "textAutoResize": "WIDTH_AND_HEIGHT",
                "x": 269.44, "y": 214.0, "width": 45.0, "height": 14.0}))
        ]
    })
}

struct Fixture;

#[async_trait]
impl FigmaUpstream for Fixture {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["get_metadata".to_owned(), "use_figma".to_owned()])
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        Ok(UpstreamResult {
            raw: match call.tool_name() {
                "get_metadata" => json!({"structuredContent": {"devupMetadata": {
                    "fileKey": "WidthKey", "version": "v1", "rootId": "2:0",
                    "nodes": [{"id": "2:0", "type": "FRAME", "name": "Screen",
                        "childrenIds": [], "descendantCount": 4}]
                }}}),
                _ => snapshot(),
            },
        })
    }
}

async fn export(arguments: Value) -> anyhow::Result<(bool, Value)> {
    let server = DevupServer::new(Services::new(Arc::new(Auth), Arc::new(Fixture)));
    let (server_transport, client_transport) = tokio::io::duplex(512 * 1024);
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
    Ok((
        result.is_error == Some(true),
        result.structured_content.unwrap_or_default(),
    ))
}

const URL: &str = "https://www.figma.com/design/WidthKey/Screen?node-id=2-0";

/// The validator joins its violations to the sourceMap exactly as the export
/// returns it - a screen's entries sit under `tsx`, not `entries`. A map
/// rebuilt by hand in a test proved nothing about that shape.
#[tokio::test]
async fn violations_link_to_the_sourcemap_the_export_returns() -> anyhow::Result<()> {
    let server = DevupServer::new(Services::new(Arc::new(Auth), Arc::new(Fixture)));
    let (server_transport, client_transport) = tokio::io::duplex(1024 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let call = |name: &'static str, arguments: Value| {
        let client = &client;
        async move {
            let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap();
            let result = client
                .call_tool(CallToolRequestParams::new(name).with_arguments(arguments))
                .await?;
            anyhow::Ok(result.structured_content.unwrap_or_default())
        }
    };
    let exported = call(
        "devup_figma_export",
        json!({"url": URL, "outputs": ["tsx", "sourceMap"]}),
    )
    .await?;
    let validated = call(
        "devup_ui_validate",
        json!({"tsx": exported["tsx"], "sourceMap": exported["sourceMap"]}),
    )
    .await?;
    let link = &validated["sourceMapLink"];
    assert_eq!(link["state"], "linked", "{validated}");
    assert!(link["linkedViolations"].as_u64().unwrap() > 0, "{link}");
    let colour = validated["violations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|violation| violation["rule"] == "hardcoded-color")
        .expect("a hardcoded fill");
    assert_eq!(colour["source"][0]["property"], "fills", "{colour}");
    client.cancel().await?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn the_default_export_reports_width_behavior_and_reuse() -> anyhow::Result<()> {
    let (failed, output) = export(json!({"url": URL})).await?;
    assert!(!failed, "{output}");
    let tsx = output["tsx"].as_str().unwrap();
    assert!(tsx.contains("w=\"360px\""), "{tsx}");
    assert!(tsx.contains("left=\"259.89px\""), "{tsx}");

    let behavior = &output["widthBehavior"];
    assert_eq!(behavior["designWidth"], 360.0);
    assert_eq!(behavior["root"]["fluid"], false);
    assert_eq!(behavior["verification"]["runtime"], "not-performed");
    let badge = behavior["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["nodeId"] == "2:6")
        .expect("the badge is reported");
    assert_eq!(badge["kind"], "right-aligned-but-held-left");
    assert_eq!(badge["carries"], json!(["2:7"]));

    let reuse = &output["cache"]["reuse"];
    assert!(
        reuse["withArtifactId"]
            .as_array()
            .unwrap()
            .contains(&json!("tsx"))
    );
    assert!(
        reuse["needsUrl"]
            .as_array()
            .unwrap()
            .contains(&json!("referencePng"))
    );

    let scope = &output["verificationScope"];
    assert_eq!(scope["collectedWidths"], json!([360.0]), "{scope}");
    assert_eq!(scope["rendered"]["state"], "not-performed");
    Ok(())
}

#[tokio::test]
async fn a_width_policy_reanchors_and_an_unknown_node_is_refused() -> anyhow::Result<()> {
    let (failed, output) = export(json!({
        "url": URL,
        "widthPolicy": {"root": "fluid", "anchors": {"2:4": "stretch", "2:6": "right"}}
    }))
    .await?;
    assert!(!failed, "{output}");
    let tsx = output["tsx"].as_str().unwrap();
    assert!(tsx.contains("w=\"100%\""), "{tsx}");
    assert!(tsx.contains("right=\"36px\""), "{tsx}");
    assert!(
        tsx.contains("right=\"45.56px\""),
        "the badge text follows: {tsx}"
    );
    assert!(tsx.contains("right=\"20px\""), "the card stretches: {tsx}");
    assert_eq!(output["widthBehavior"]["widthPolicy"]["applied"], true);

    let (failed, error) = export(json!({
        "url": URL,
        "widthPolicy": {"root": "fluid", "anchors": {"9:9": "right"}}
    }))
    .await?;
    assert!(failed);
    assert_eq!(error["error"]["code"], "DEVUP_INVALID_INPUT", "{error}");
    assert_eq!(
        error["error"]["details"]["unknownNodeIds"],
        json!(["9:9"]),
        "{error}"
    );

    let (failed, error) = export(json!({
        "url": URL,
        "widthPolicy": {"root": "fixed"}
    }))
    .await?;
    assert!(failed, "{error}");
    Ok(())
}
