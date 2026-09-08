//! A screen captured with its other widths comes back as one tree.
//!
//! This drives the real export tool over a stubbed upstream, so it covers the
//! whole path: the fast snapshot envelope, the projection, and the module the
//! caller is handed.

use std::sync::Arc;

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{AuthStatus, DevupError, FigmaUpstream, ReadToolCall, UpstreamResult};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};

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

/// One frame of a screen: an outer stack padded on both sides, a card of two
/// lines inside it, and on the wider width an extra panel the narrow one does
/// not draw.
fn width(root: &str, name: &str, pixels: u32, padding: u32, gap: u32, aside: bool) -> Vec<Value> {
    let card = format!("{root}:card");
    let panel = format!("{root}:aside");
    let lines = [format!("{root}:line1"), format!("{root}:line2")];
    let mut children = vec![card.clone()];
    if aside {
        children.push(panel.clone());
        children.push(format!("{root}:note"));
    }
    let text = |id: &str, characters: &str| {
        json!({
            "id": id,
            "type": "TEXT",
            "fields": {
                "name": characters,
                "childrenIds": [],
                "characters": characters,
                "width": 200,
                "height": 24
            },
            "extra": {},
            "fieldErrors": {}
        })
    };
    let mut nodes = vec![
        json!({
            "id": root,
            "type": "FRAME",
            "fields": {
                "name": name,
                "parentId": "0:section",
                "parentType": "SECTION",
                "parentName": "notice board",
                "childrenIds": children,
                "layoutMode": "VERTICAL",
                "width": pixels,
                "height": 800,
                "inferredAutoLayout": {
                    "layoutMode": "VERTICAL",
                    "paddingTop": 0,
                    "paddingBottom": 0,
                    "paddingLeft": padding,
                    "paddingRight": padding,
                    "itemSpacing": 0
                }
            },
            "extra": {},
            "fieldErrors": {}
        }),
        json!({
            "id": card,
            "type": "FRAME",
            "fields": {
                "name": "card",
                "childrenIds": lines,
                "layoutMode": "VERTICAL",
                "width": pixels - padding * 2,
                "height": 200,
                "inferredAutoLayout": {"layoutMode": "VERTICAL", "itemSpacing": gap}
            },
            "extra": {},
            "fieldErrors": {}
        }),
        text(&lines[0], "first"),
        text(&lines[1], "second"),
    ];
    if aside {
        // A row rather than a column, so its shape is nothing like the card's.
        // Children are paired by shape where that is unambiguous, and two
        // frames with the same shape would be told apart by name instead.
        nodes.push(json!({
            "id": panel,
            "type": "FRAME",
            "fields": {
                "name": "aside",
                "childrenIds": [],
                "layoutMode": "HORIZONTAL",
                "width": 300,
                "height": 120,
                "inferredAutoLayout": {"layoutMode": "HORIZONTAL", "itemSpacing": 0}
            },
            "extra": {},
            "fieldErrors": {}
        }));
        // No layout mode, so this one is a plain `Box`. It is here to pin what
        // a hidden-then-shown node is given back: a `div` is `block`, and
        // `display: initial` would make it `inline`.
        nodes.push(json!({
            "id": format!("{root}:note"),
            "type": "FRAME",
            "fields": {
                "name": "note",
                "childrenIds": [],
                "width": 300,
                "height": 40
            },
            "extra": {},
            "fieldErrors": {}
        }));
    }
    nodes
}

#[derive(Debug)]
struct TwoWidths;

#[async_trait]
impl FigmaUpstream for TwoWidths {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        match call {
            ReadToolCall::Snapshot { .. } => {
                let mut nodes = width("1:10", "mobile", 360, 16, 20, false);
                nodes.extend(width("1:20", "desktop", 1920, 40, 30, true));
                let node_count = nodes.len();
                let mut envelope = json!({
                    "kind": "devupFastSnapshotEnvelope",
                    "schemaVersion": 1,
                    "source": {"fileKey": "FileKey123", "rootId": "1:20"},
                    "snapshot": {
                        "fileKey": "FileKey123",
                        "version": "v1",
                        "rootIds": ["1:10", "1:20"],
                        "nodes": nodes,
                        "diagnostics": []
                    },
                    "resources": {
                        "collections": [],
                        "variables": [],
                        "styles": [],
                        "usedVariableIds": [],
                        "usedStyleIds": [],
                        "usedRemoteVariables": [],
                        "localComplete": true,
                        "usedRemoteComplete": true,
                        "unresolved": []
                    },
                    "integrity": {
                        "nodeCount": node_count,
                        "variableRefCount": 0,
                        "styleRefCount": 0,
                        "utf8Bytes": 0
                    }
                });
                // The envelope declares its own length, so it has to be
                // serialised until the declaration matches.
                loop {
                    let bytes = serde_json::to_vec(&envelope).unwrap_or_default().len();
                    if envelope["integrity"]["utf8Bytes"] == bytes as u64 {
                        break;
                    }
                    envelope["integrity"]["utf8Bytes"] = Value::from(bytes);
                }
                Ok(UpstreamResult {
                    raw: json!({"content": [
                        {"type": "text", "text": envelope.to_string()}
                    ]}),
                })
            }
            ReadToolCall::Metadata { .. } => Ok(UpstreamResult {
                raw: json!({"content": [{"type": "text", "text": json!({
                    "fileKey": "FileKey123",
                    "version": "v1",
                    "rootId": "1:20",
                    "nodes": [
                        {"id": "1:10", "type": "FRAME", "name": "mobile",
                         "childrenIds": ["1:10:card"], "descendantCount": 1},
                        {"id": "1:20", "type": "FRAME", "name": "desktop",
                         "childrenIds": ["1:20:card", "1:20:aside"], "descendantCount": 2}
                    ]
                }).to_string()}]}),
            }),
            // Nothing in this screen is an asset or an image, so anything else
            // the export asks for is answered empty rather than refused.
            _ => Ok(UpstreamResult {
                raw: json!({"content": []}),
            }),
        }
    }
}

async fn export(arguments: Value) -> anyhow::Result<Value> {
    let server = DevupServer::new(Services::new(Arc::new(ConnectedAuth), Arc::new(TwoWidths)));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let arguments = arguments.as_object().cloned().unwrap_or_default();
    let result = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_export".to_owned()).with_arguments(arguments),
        )
        .await?;
    let structured = result
        .structured_content
        .clone()
        .ok_or_else(|| anyhow::anyhow!("no structured content"))?;
    client.cancel().await?;
    task.await??;
    Ok(structured)
}

#[tokio::test]
async fn a_screen_with_other_widths_is_exported_as_one_responsive_module() -> anyhow::Result<()> {
    let result = export(json!({
        "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-20",
        "outputs": ["tsx"],
        "componentName": "Notice"
    }))
    .await?;

    let written = result["responsiveTsx"]
        .as_str()
        .expect("a screen drawn at two widths has a responsive module");
    // Arrays are written one slot to a line; fold them so the expectations
    // below can be read in one.
    let module = folded(written);

    // 360 goes in the first slot and 1920 in the last, so a value that differs
    // between them is written at both ends of the array.
    assert_eq!(
        result["responsiveSlots"].as_array().map(Vec::len),
        Some(2),
        "{module}"
    );
    assert!(
        module.contains(r#"px={["16px", null, null, null, "40px"]}"#),
        "the outer padding should have merged:\n{module}"
    );
    assert!(
        module.contains(r#"gap={["20px", null, null, null, "30px"]}"#),
        "the card's spacing should have merged:\n{module}"
    );
    // The panel is drawn at one width only, so it is kept and toggled.
    assert!(
        module.contains(r#"display={["none", null, null, null, "flex"]}"#),
        "the wide-only panel should be toggled:\n{module}"
    );
    // And what a hidden node is given back is what the element is, not
    // `initial` — for `display` that is `inline` however the element is drawn,
    // so a `Box` would come back inline instead of block.
    assert!(
        module.contains(r#"display={["none", null, null, null, "block"]}"#),
        "a wide-only Box should come back as a block:\n{module}"
    );
    assert!(
        !module.contains("initial"),
        "display is never cleared to initial:\n{module}"
    );

    // It is a module, not a fragment: it imports what it uses and exports the
    // screen.
    assert!(module.starts_with("import { "));
    assert!(module.contains("} from '@devup-ui/react'"));
    assert!(module.contains("export default function Notice() {"));
    assert!(module.trim_end().ends_with('}'));
    // The import list used to be repeated back as `responsiveImports` and
    // `responsiveComponents`, which is the module's own first line said
    // twice. Read it off the module instead, which is the thing that has to
    // be right.
    let imports = module
        .lines()
        .next()
        .expect("a module opens with its import");
    assert_eq!(
        imports, "import { Box, Flex, Text, VStack } from '@devup-ui/react'",
        "only the elements it actually uses, and no component of its own"
    );
    Ok(())
}

/// It is an output in its own right, not something that only falls out of
/// asking for `tsx`.
#[tokio::test]
async fn the_responsive_module_can_be_asked_for_on_its_own() -> anyhow::Result<()> {
    let result = export(json!({
        "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-20",
        "outputs": ["responsiveTsx"],
        "componentName": "Notice"
    }))
    .await?;
    assert!(
        result["responsiveTsx"]
            .as_str()
            .is_some_and(|module| module.contains("export default function Notice() {")),
        "{result:#?}"
    );
    assert!(
        result.get("tsx").is_none(),
        "asking for one output should not produce the other"
    );
    Ok(())
}

/// Without a name from the caller the page is named as the plugin names it:
/// after the Section the widths sit in, in PascalCase, with `Page` on the
/// end.
#[tokio::test]
async fn the_page_is_named_after_its_section_when_the_caller_gives_no_name() -> anyhow::Result<()> {
    let result = export(json!({
        "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-20",
        "outputs": ["responsiveTsx"]
    }))
    .await?;
    assert!(
        result["responsiveTsx"]
            .as_str()
            .is_some_and(|module| module.contains("export default function NoticeBoardPage() {")),
        "{result:#?}"
    );
    Ok(())
}

/// A responsive array written one slot to a line, folded back onto one.
fn folded(tsx: &str) -> String {
    let mut out = String::with_capacity(tsx.len());
    let mut items: Option<Vec<String>> = None;
    for line in tsx.lines() {
        let trimmed = line.trim();
        match &mut items {
            Some(collected) => {
                if let Some(rest) = trimmed.strip_prefix("]}") {
                    out.push_str(&collected.join(", "));
                    out.push_str("]}");
                    out.push_str(rest);
                    out.push('\n');
                    items = None;
                } else {
                    collected.push(trimmed.trim_end_matches(',').to_owned());
                }
            }
            None => {
                out.push_str(line);
                if trimmed.ends_with("={[") {
                    items = Some(Vec::new());
                } else {
                    out.push('\n');
                }
            }
        }
    }
    out
}
