//! Raw wire-protocol regression test for the "opencode can't load devup-mcp"
//! incompatibility.
//!
//! Root cause: `devup_figma_continue`'s `result` field is typed
//! `serde_json::Value` so it can carry an arbitrary host-executed Figma MCP
//! response. schemars' blanket `JsonSchema` impl for `Value` serializes that
//! to the JSON Schema 2020-12 boolean shorthand `true` ("accept anything").
//! That is spec-legal, but several MCP clients' schema converters (opencode
//! included) assume every `properties` entry is a JSON object and reject the
//! *entire* `tools/list` response — not just the one field — when they hit a
//! bare boolean. Every `outputSchema` also lacked the `"type": "object"`
//! marker the spec (SEP-2106) requires.
//!
//! `stdio_tools.rs` exercises the same server through rmcp's own in-process
//! client, but that client deserializes `Tool.input_schema`/`output_schema`
//! into typed `Arc<JsonObject>` before test code can inspect them — a
//! boolean schema and an equivalent object schema round-trip identically
//! through that path, so a regression there would not be caught. This test
//! instead spawns the actual compiled `devup-mcp` binary over real OS pipes
//! and parses the raw `tools/list` JSON exactly as an external client would,
//! so a reintroduced boolean schema anywhere fails here.

use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use serde_json::{Value, json};

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
/// at `path`) where a bare boolean (`true`/`false`) appears where the spec
/// expects a schema. Returns the dotted path of each hit, or an empty `Vec`
/// if none are found.
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

/// A minimal, dependency-free JSON-RPC-over-newline-delimited-stdio client,
/// deliberately independent of rmcp's own client so this test observes the
/// exact bytes an external MCP client would.
struct RawStdioClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl RawStdioClient {
    fn spawn() -> anyhow::Result<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_devup-mcp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child.stdin.take().expect("devup-mcp stdin must be piped");
        let stdout = BufReader::new(child.stdout.take().expect("devup-mcp stdout must be piped"));
        Ok(Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        })
    }

    fn call(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;
        loop {
            let response = self.read_message()?;
            if response.get("id") == Some(&Value::from(id)) {
                return Ok(response);
            }
            // Frames with a different (or absent) id are notifications /
            // unrelated responses; keep reading until ours arrives.
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> anyhow::Result<()> {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn send(&mut self, message: &Value) -> anyhow::Result<()> {
        let mut line = serde_json::to_string(message)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.flush()?;
        Ok(())
    }

    fn read_message(&mut self) -> anyhow::Result<Value> {
        let mut line = String::new();
        loop {
            line.clear();
            let read = self.stdout.read_line(&mut line)?;
            anyhow::ensure!(read > 0, "devup-mcp closed stdout before responding");
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            return Ok(serde_json::from_str(trimmed)?);
        }
    }
}

impl Drop for RawStdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn tools_list_over_raw_stdio_has_no_boolean_schemas_and_object_output_types() -> anyhow::Result<()>
{
    let mut client = RawStdioClient::spawn()?;

    let initialize = client.call(
        "initialize",
        json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "stdio-smoke-test", "version": "0.0.0" },
        }),
    )?;
    anyhow::ensure!(
        initialize.get("error").is_none(),
        "initialize failed: {initialize}"
    );

    client.notify("notifications/initialized", json!({}))?;

    let tools_list = client.call("tools/list", json!({}))?;
    anyhow::ensure!(
        tools_list.get("error").is_none(),
        "tools/list failed: {tools_list}"
    );

    let tools = tools_list
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .expect("tools/list result must contain a tools array");
    assert_eq!(
        tools.len(),
        9,
        "expected all 9 devup-mcp tools (6 devup_figma_* + devup_project_context + devup_ui_validate + devup_stack_diff) to be listed: {tools:?}"
    );

    let mut boolean_schema_hits = Vec::new();
    let mut missing_object_output_type = Vec::new();

    for tool in tools {
        let name = tool
            .get("name")
            .and_then(Value::as_str)
            .expect("each tool must have a name");

        if let Some(input_schema) = tool.get("inputSchema") {
            boolean_schema_hits.extend(find_boolean_schemas(
                &format!("{name}.inputSchema"),
                input_schema,
            ));
        }

        let output_schema = tool
            .get("outputSchema")
            .unwrap_or_else(|| panic!("{name} is missing outputSchema"));
        boolean_schema_hits.extend(find_boolean_schemas(
            &format!("{name}.outputSchema"),
            output_schema,
        ));

        // MCP spec (SEP-2106): a declared `outputSchema` must describe an
        // object (`"type": "object"`).
        if output_schema.get("type") != Some(&json!("object")) {
            missing_object_output_type.push(name.to_owned());
        }
    }

    assert!(
        boolean_schema_hits.is_empty(),
        "found boolean JSON Schema(s) at: {boolean_schema_hits:?}. Many MCP \
         clients (opencode included) reject the *entire* tools/list response \
         when any schema position is a bare `true`/`false` instead of an \
         object."
    );
    assert!(
        missing_object_output_type.is_empty(),
        "outputSchema missing \"type\": \"object\" for: {missing_object_output_type:?}"
    );

    Ok(())
}
