use devup_mcp::server::DevupServer;
use rmcp::ServiceExt;
use serde_json::Value;

/// JSON Schema keywords whose value is itself a single (sub-)schema.
const SINGLE_SCHEMA_KEYS: &[&str] = &[
    "items",
    "additionalProperties",
    "additionalItems",
    "contains",
    "propertyNames",
    "not",
    "if",
    "then",
    "else",
    "unevaluatedItems",
    "unevaluatedProperties",
];

/// JSON Schema keywords whose value is a map of name -> schema.
const MAP_SCHEMA_KEYS: &[&str] = &[
    "properties",
    "patternProperties",
    "$defs",
    "definitions",
    "dependentSchemas",
];

/// JSON Schema keywords whose value is an array of schemas.
const LIST_SCHEMA_KEYS: &[&str] = &["allOf", "anyOf", "oneOf", "prefixItems"];

/// Recursively finds every position in `node` (a JSON Schema document rooted
/// at `path`) where a bare boolean (`true`/`false`) appears where the JSON
/// Schema 2020-12 spec expects a schema. Such booleans are spec-legal
/// shorthand ("accept"/"reject" anything), but several MCP clients'
/// tool-schema converters (opencode included) reject the entire `tools/list`
/// response when they encounter one instead of an object, which is exactly
/// how `devup_figma_continue`'s `serde_json::Value`-typed `result` field
/// broke opencode compatibility. See `stdio_smoke.rs` for the raw-wire
/// regression test that exercises this against the compiled binary.
fn find_boolean_schemas(path: &str, node: &Value) -> Vec<String> {
    let mut hits = Vec::new();
    collect_boolean_schemas(path, node, &mut hits);
    hits
}

fn collect_boolean_schemas(path: &str, node: &Value, hits: &mut Vec<String>) {
    if node.is_boolean() {
        hits.push(path.to_owned());
        return;
    }
    let Some(object) = node.as_object() else {
        return;
    };
    for key in SINGLE_SCHEMA_KEYS {
        if let Some(child) = object.get(*key) {
            collect_boolean_schemas(&format!("{path}.{key}"), child, hits);
        }
    }
    for key in MAP_SCHEMA_KEYS {
        if let Some(Value::Object(map)) = object.get(*key) {
            for (name, child) in map {
                collect_boolean_schemas(&format!("{path}.{key}.{name}"), child, hits);
            }
        }
    }
    for key in LIST_SCHEMA_KEYS {
        if let Some(Value::Array(items)) = object.get(*key) {
            for (index, child) in items.iter().enumerate() {
                collect_boolean_schemas(&format!("{path}.{key}[{index}]"), child, hits);
            }
        }
    }
}

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

    let mut boolean_schema_hits = Vec::new();
    let mut missing_object_output_type = Vec::new();
    for tool in &tools {
        let input_schema = Value::Object((*tool.input_schema).clone());
        boolean_schema_hits.extend(find_boolean_schemas(
            &format!("{}.inputSchema", tool.name),
            &input_schema,
        ));
        let Some(output_schema) = tool.output_schema.as_deref() else {
            continue;
        };
        let output_schema = Value::Object(output_schema.clone());
        boolean_schema_hits.extend(find_boolean_schemas(
            &format!("{}.outputSchema", tool.name),
            &output_schema,
        ));
        if output_schema.get("type") != Some(&serde_json::json!("object")) {
            missing_object_output_type.push(tool.name.to_string());
        }
    }
    assert!(
        boolean_schema_hits.is_empty(),
        "found boolean JSON Schema(s) at: {boolean_schema_hits:?}. A bare \
         `true`/`false` anywhere in inputSchema/outputSchema makes several \
         MCP clients (opencode included) drop the entire tools/list response."
    );
    assert!(
        missing_object_output_type.is_empty(),
        "outputSchema missing \"type\": \"object\" (MCP spec SEP-2106) for: \
         {missing_object_output_type:?}"
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
    assert!(explore_text.contains("refresh"));
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
