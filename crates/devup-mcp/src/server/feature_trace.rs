//! Explicit-anchor, read-only cross-layer evidence. No requirement interpretation.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, generate_component},
    provenance::design_fingerprints,
};
use devup_mcp_figma::{DevupError, ErrorCode};
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use serde_json::{Value, json};

use super::{
    artifacts::ArtifactStore, project_context, project_root, stack_diff, tools::FeatureTraceInput,
};

const MAX_BYTES: usize = 65_536;
const MAX_SOURCE_BYTES: u64 = 2_097_152;
const METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "head", "options", "trace",
];
const ACCEPTED: &str = "Supply at least one explicit anchor: routePath, figmaNodeId, artifactId, operationId, apiPath with method, componentPath, or tableName. Requirement prose is never interpreted as an anchor.";

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}
fn array(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn supplied(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|s| !s.trim().is_empty())
}
fn invalid(message: &str) -> DevupError {
    DevupError::new(ErrorCode::DevupInvalidInput, message, false)
}
fn read(path: &Path) -> Result<String, String> {
    let mut source = String::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_string(&mut source)
        .map_err(|e| e.to_string())?;
    if source.len() as u64 > MAX_SOURCE_BYTES {
        return Err("maxSourceBytes exceeded; source was not parsed".into());
    }
    Ok(source)
}
fn relative(root: &Path, file: &Path) -> String {
    project_context::relative_display(root, file)
}
fn hop(kind: &str, from: Value, to: Value, evidence: Value, reason: Option<&str>) -> Value {
    json!({"kind":kind,"from":from,"to":to,"status":if reason.is_some(){"UNVERIFIED"}else{"RESOLVED"},
        "confidence":if reason.is_some(){"low"}else{"medium"},"evidence":evidence,"reason":reason})
}

/// Apply the shared ownership patterns to concrete paths, including nested
/// authorities. Authored-looking files inside generated trees stay protected.
fn generated_path(path: &str, ownership: &Value) -> bool {
    let wrapped = format!("/{path}");
    array(&ownership["generated"]).iter().any(|entry| {
        let pattern = text(&entry["path"]);
        if let Some(directory) = pattern.strip_suffix("/**") {
            wrapped.contains(&format!("/{directory}/"))
        } else if let Some((directory, suffix)) = pattern.split_once("/**/") {
            wrapped.contains(&format!("/{directory}/"))
                && path.ends_with(suffix.trim_start_matches('*'))
        } else {
            path == pattern || wrapped.ends_with(&format!("/{pattern}"))
        }
    })
}
fn authored_artifact(path: &str, ownership: &Value) -> Value {
    let generated = generated_path(path, ownership);
    json!({"path":path,"ownership":if generated{"generated"}else{"human-authored"},
        "editTarget":if generated{None}else{Some(path)},"sourceOwnership":ownership})
}

struct Operation {
    endpoint: Value,
    operation: Value,
    spec: Arc<Value>,
    file: String,
}
struct Handler {
    file: PathBuf,
    source: String,
    method: String,
    path: String,
}

fn operations(root: &Path, api: &Value, diagnostics: &mut Vec<Value>) -> Vec<Operation> {
    let mut result = vec![];
    for entry in array(&api["specs"]) {
        let file = text(&entry["path"]);
        let spec = match read(&root.join(file))
            .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()))
        {
            Ok(spec) => spec,
            Err(reason) => {
                diagnostics.push(json!({"path":file,"reason":reason}));
                continue;
            }
        };
        let spec = Arc::new(spec);
        for endpoint in array(&entry["endpoints"]) {
            let operation = &spec["paths"][text(&endpoint["path"])]
                [text(&endpoint["method"]).to_ascii_lowercase()];
            result.push(Operation {
                endpoint: endpoint.clone(),
                operation: operation.clone(),
                spec: spec.clone(),
                file: file.into(),
            });
        }
    }
    result
}
fn handlers(root: &Path, diagnostics: &mut Vec<Value>) -> Vec<Handler> {
    let (directories, excluded) = project_root::find_dirs_named(root, "routes", 5);
    diagnostics.extend(excluded);
    let mut result = vec![];
    let mut seen = BTreeSet::new();
    for directory in directories {
        let (files, excluded) = project_root::find_matching_files(
            &directory,
            |p| p.extension().is_some_and(|s| s == "rs"),
            6,
        );
        diagnostics.extend(excluded);
        for file in files {
            if !seen.insert(file.clone()) {
                continue;
            }
            let source = match read(&file) {
                Ok(s) => s,
                Err(reason) => {
                    diagnostics.push(json!({"path":relative(root,&file),"reason":reason}));
                    continue;
                }
            };
            let prefix = stack_diff::route_url_prefix(file.strip_prefix(&directory).unwrap());
            for (method, path) in stack_diff::extract_vespera_route_attributes(&source) {
                result.push(Handler {
                    file: file.clone(),
                    source: source.clone(),
                    method,
                    path: stack_diff::join_route_url(&prefix, path.as_deref()),
                });
            }
        }
    }
    result
}

#[derive(Default)]
struct Design {
    names: BTreeSet<String>,
    fields: BTreeSet<String>,
    states: BTreeSet<String>,
    diagnostics: Vec<Value>,
    evidence: Value,
}
impl Design {
    fn expression(&mut self, e: &Expression<'_>, source: &str) {
        match e {
            Expression::JSXElement(e) => self.element(e, source),
            Expression::JSXFragment(e) => self.children(&e.children, source),
            Expression::ParenthesizedExpression(e) => self.expression(&e.expression, source),
            Expression::ArrowFunctionExpression(e) => match &e.body {
                ArrowFunctionBody::FunctionBody(body) => self.statements(&body.statements, source),
                other => {
                    if let Some(e) = other.as_expression() {
                        self.expression(e, source);
                    }
                }
            },
            Expression::FunctionExpression(e) => {
                if let Some(body) = &e.body {
                    self.statements(&body.statements, source);
                }
            }
            Expression::ConditionalExpression(e) => {
                self.expression(&e.consequent, source);
                self.expression(&e.alternate, source);
            }
            Expression::LogicalExpression(e) => {
                self.expression(&e.left, source);
                self.expression(&e.right, source);
            }
            _ => {}
        }
    }
    fn declaration(&mut self, d: &Declaration<'_>, source: &str) {
        match d {
            Declaration::FunctionDeclaration(f) => {
                if let Some(body) = &f.body {
                    self.statements(&body.statements, source);
                }
            }
            Declaration::VariableDeclaration(v) => {
                for d in &v.declarations {
                    if let Some(e) = &d.init {
                        self.expression(e, source);
                    }
                }
            }
            _ => {}
        }
    }
    fn statements(&mut self, statements: &[Statement<'_>], source: &str) {
        for s in statements {
            match s {
                Statement::ExportDeclaration(e) => self.declaration(&e.declaration, source),
                Statement::ExportDefaultDeclaration(e) => match &e.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                        if let Some(body) = &f.body {
                            self.statements(&body.statements, source);
                        }
                    }
                    other => {
                        if let Some(e) = other.as_expression() {
                            self.expression(e, source);
                        }
                    }
                },
                Statement::FunctionDeclaration(f) => {
                    if let Some(body) = &f.body {
                        self.statements(&body.statements, source);
                    }
                }
                Statement::VariableDeclaration(v) => {
                    for d in &v.declarations {
                        if let Some(e) = &d.init {
                            self.expression(e, source);
                        }
                    }
                }
                Statement::ReturnStatement(s) => {
                    if let Some(e) = &s.argument {
                        self.expression(e, source);
                    }
                }
                Statement::ExpressionStatement(e) => self.expression(&e.expression, source),
                Statement::BlockStatement(b) => self.statements(&b.body, source),
                _ => {}
            }
        }
    }
    fn children(&mut self, children: &[JSXChild<'_>], source: &str) {
        for child in children {
            match child {
                JSXChild::Element(e) => self.element(e, source),
                JSXChild::Fragment(e) => self.children(&e.children, source),
                JSXChild::ExpressionContainer(e) => {
                    if let Some(e) = e.expression.as_expression() {
                        self.expression(e, source);
                    }
                }
                _ => {}
            }
        }
    }
    fn element(&mut self, element: &JSXElement<'_>, source: &str) {
        let opening = &element.opening_element;
        let name = opening.name.span().source_text(source);
        if name.chars().next().is_some_and(char::is_uppercase) {
            self.names.insert(name.into());
        }
        for attribute in &opening.attributes {
            let JSXAttributeItem::Attribute(a) = attribute else {
                self.diagnostics.push(
                    json!({"reason":"Spread JSX props may contain additional field bindings."}),
                );
                continue;
            };
            let key = a.name.span().source_text(source);
            if key == "data-state" {
                if let Some(JSXAttributeValue::StringLiteral(value)) = &a.value
                    && ["loading", "error", "empty", "validation", "authorization"]
                        .contains(&value.value.as_str())
                {
                    self.states.insert(value.value.to_string());
                }
                continue;
            }
            if !matches!(key, "name" | "data-field") {
                continue;
            }
            if let Some(JSXAttributeValue::StringLiteral(value)) = &a.value {
                self.fields.insert(value.value.to_string());
            } else {
                self.diagnostics.push(
                    json!({"reason":"Nonliteral field binding is unresolved.","attribute":key}),
                );
            }
        }
        self.children(&element.children, source);
    }
}
fn design_source(source: &str) -> Design {
    let mut design = Design::default();
    if source.len() as u64 > MAX_SOURCE_BYTES {
        design
            .diagnostics
            .push(json!({"reason":"maxSourceBytes exceeded"}));
        return design;
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.diagnostics.is_empty() {
        design
            .diagnostics
            .push(json!({"reason":"componentTsx could not be parsed; no bindings extracted."}));
        return design;
    }
    design.statements(&parsed.program.body, source);
    design
}
async fn design(input: &FeatureTraceInput, artifacts: &ArtifactStore) -> Design {
    if supplied(&input.artifact_id) {
        if let Some(artifact) = artifacts.get(input.artifact_id.as_deref().unwrap()).await {
            let node = input.figma_node_id.as_deref().or(artifact
                .payload
                .target
                .node_id
                .as_deref());
            if let Some(node) = node {
                match generate_component(
                    &artifact.payload.snapshot,
                    node,
                    &CodegenOptions::default().with_payload_tokens(&artifact.payload),
                ) {
                    Ok(output) => {
                        let mut design = design_source(&output.tsx);
                        design.evidence = json!({"source":"cached-artifact","artifactId":artifact.artifact_id,"nodeId":node,"sourceMap":output.source_map,"designFingerprints":design_fingerprints(&artifact.payload.snapshot),"staleness":"unknown: cached snapshot has not been compared with live Figma"});
                        return design;
                    }
                    Err(e) => {
                        return Design {
                            diagnostics: vec![json!({"reason":e.to_string()})],
                            ..Design::default()
                        };
                    }
                }
            }
            return Design {
                diagnostics: vec![
                    json!({"reason":"Artifact does not identify a single node; supply figmaNodeId."}),
                ],
                ..Design::default()
            };
        }
        return Design {
            diagnostics: vec![
                json!({"reason":"Artifact missing or expired; export the explicit Figma selection again."}),
            ],
            ..Design::default()
        };
    }
    if supplied(&input.figma_node_id)
        && let Some(source) = &input.component_tsx
    {
        let mut design = design_source(source);
        design.evidence = json!({"source":"caller-supplied-componentTsx","nodeId":input.figma_node_id,"provenance":"UNVERIFIED: caller associates this export with the node; no cached artifact validates that association"});
        return design;
    }
    Design {
        diagnostics: vec![
            json!({"reason":"No design export available. A node id alone does not contain design data; supply artifactId or componentTsx."}),
        ],
        ..Design::default()
    }
}

fn schema_fields(
    schema: &Value,
    spec: &Value,
    fields: &mut BTreeSet<String>,
    reasons: &mut Vec<String>,
    depth: usize,
) {
    if depth > 16 {
        reasons.push("Schema reference/depth cap maxSchemaDepth (16) reached".into());
        return;
    }
    if let Some(reference) = schema["$ref"].as_str() {
        if let Some(target) = reference.strip_prefix('#').and_then(|p| spec.pointer(p)) {
            schema_fields(target, spec, fields, reasons, depth + 1);
        } else {
            reasons.push(format!("Unresolved schema reference {reference}"));
        }
        return;
    }
    if let Some(props) = schema["properties"].as_object() {
        fields.extend(props.keys().cloned());
    }
    for part in array(&schema["allOf"]) {
        schema_fields(part, spec, fields, reasons, depth + 1);
    }
    if schema.get("oneOf").is_some() || schema.get("anyOf").is_some() {
        reasons.push("Union schema alternatives do not prove a single field set".into());
    }
    if let Some(items) = schema.get("items") {
        schema_fields(items, spec, fields, reasons, depth + 1);
    }
    if schema
        .get("additionalProperties")
        .is_some_and(|v| v != false)
    {
        reasons.push("Open-ended additionalProperties prevents proving missing fields".into());
    }
}
fn content_fields(
    value: &Value,
    spec: &Value,
    fields: &mut BTreeSet<String>,
    reasons: &mut Vec<String>,
) {
    if let Some(reference) = value["$ref"].as_str() {
        if let Some(target) = reference.strip_prefix('#').and_then(|p| spec.pointer(p)) {
            if target.get("$ref").is_some() {
                reasons.push("Chained request/response reference unresolved".into());
            } else {
                content_fields(target, spec, fields, reasons);
            }
        } else {
            reasons.push(format!("Unresolved request/response reference {reference}"));
        }
        return;
    }
    if let Some(content) = value["content"].as_object() {
        if content.len() > 1 {
            reasons.push("Multiple media types may have different field sets".into());
        }
        for entry in content.values() {
            schema_fields(&entry["schema"], spec, fields, reasons, 0);
        }
    } else {
        reasons.push("No explicit content schema provided".into());
    }
}
fn comparison(fields: &BTreeSet<String>, contract: &BTreeSet<String>, reasons: &[String]) -> Value {
    json!({"status":if fields.is_empty() || !reasons.is_empty(){"UNVERIFIED"}else{"RESOLVED"},
        "designFields":fields,"contractFields":contract,"designFieldsNotProvided":fields.difference(contract).collect::<Vec<_>>(),
        "contractFieldsUnused":contract.difference(fields).collect::<Vec<_>>(),"reasons":reasons,
        "scope":"Literal top-level name/data-field bindings versus declared schema fields; differences are candidates only when status is UNVERIFIED. This is not type compatibility or runtime serialization proof."})
}
fn compare(design: &Design, operation: &Operation) -> Value {
    let mut request = BTreeSet::new();
    let mut response = BTreeSet::new();
    let mut request_reasons = vec![];
    let mut response_reasons = vec![];
    content_fields(
        &operation.operation["requestBody"],
        &operation.spec,
        &mut request,
        &mut request_reasons,
    );
    if let Some(responses) = operation.operation["responses"].as_object() {
        let success = responses
            .iter()
            .filter(|(status, _)| status.starts_with('2'))
            .collect::<Vec<_>>();
        if success.len() != 1 {
            response_reasons.push("No unique success response schema".into());
        }
        for (_, value) in success {
            content_fields(value, &operation.spec, &mut response, &mut response_reasons);
        }
    } else {
        response_reasons.push("No response schema".into());
    }
    if design.fields.is_empty() {
        request_reasons.push(
            "Design has no literal contract field bindings; visual labels are not field names"
                .into(),
        );
        response_reasons.push("Design has no literal contract field bindings".into());
    }
    for d in &design.diagnostics {
        request_reasons.push(text(&d["reason"]).into());
        response_reasons.push(text(&d["reason"]).into());
    }
    json!({"operation":operation.endpoint,"specPath":operation.file,"designEvidence":design.evidence,"request":comparison(&design.fields,&request,&request_reasons),"response":comparison(&design.fields,&response,&response_reasons)})
}

fn bound(mut value: Value, max_items: usize) -> Value {
    let mut omitted = BTreeMap::<String, usize>::new();
    fn trim(v: &mut Value, path: &str, limit: usize, omitted: &mut BTreeMap<String, usize>) {
        match v {
            Value::Array(a) => {
                if a.len() > limit {
                    omitted.insert(path.into(), a.len() - limit);
                    a.truncate(limit);
                }
                for (i, v) in a.iter_mut().enumerate() {
                    trim(v, &format!("{path}/{i}"), limit, omitted);
                }
            }
            Value::Object(o) => {
                for (k, v) in o {
                    trim(
                        v,
                        &if path.is_empty() {
                            k.clone()
                        } else {
                            format!("{path}/{k}")
                        },
                        limit,
                        omitted,
                    );
                }
            }
            _ => {}
        }
    }
    trim(&mut value, "", max_items, &mut omitted);
    let mut caps = vec![];
    if !omitted.is_empty() {
        caps.push("maxItems");
    }
    value["limits"] = json!({"maxItems":max_items,"maxBytes":MAX_BYTES,"maxSourceBytes":MAX_SOURCE_BYTES,"maxSchemaDepth":16,"byteEncoding":"UTF-8 compact JSON before MCP envelope and server identity"});
    value["truncation"] = json!({"truncated":!caps.is_empty(),"capsHit":caps,"omitted":omitted});
    // Remove the largest payload member until the full response, including cap
    // metadata, fits. Every removal is named; never silently slice a hop's reason.
    while value.to_string().len() > MAX_BYTES {
        let key = value
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "truncation" | "limits" | "status"))
            .max_by_key(|(_, v)| v.to_string().len())
            .map(|(k, _)| k.clone());
        let Some(key) = key else {
            break;
        };
        let removed = value.as_object_mut().unwrap().remove(&key).unwrap();
        value["truncation"]["omitted"][&key] = json!({"reason":"maxBytes","items":removed.as_array().map(Vec::len),"bytes":removed.to_string().len()});
        value["truncation"]["truncated"] = json!(true);
        let caps = value["truncation"]["capsHit"].as_array_mut().unwrap();
        if !caps.contains(&json!("maxBytes")) {
            caps.push(json!("maxBytes"));
        }
    }
    value
}

pub(super) async fn run(
    input: FeatureTraceInput,
    artifacts: &ArtifactStore,
) -> Result<Value, DevupError> {
    let max_items = input.max_items.unwrap_or(100);
    if !(1..=200).contains(&max_items) {
        return Err(invalid("maxItems must be between 1 and 200."));
    }
    if supplied(&input.api_path) != supplied(&input.method) {
        return Err(invalid("apiPath and method must be supplied together."));
    }
    if supplied(&input.method)
        && !METHODS.contains(
            &input
                .method
                .as_deref()
                .unwrap()
                .to_ascii_lowercase()
                .as_str(),
        )
    {
        return Err(invalid("method must be an HTTP method."));
    }
    if ![
        &input.route_path,
        &input.figma_node_id,
        &input.artifact_id,
        &input.operation_id,
        &input.api_path,
        &input.component_path,
        &input.table_name,
    ]
    .into_iter()
    .any(supplied)
    {
        return Ok(json!({"status":"REFUSED","message":ACCEPTED}));
    }
    if input
        .component_tsx
        .as_ref()
        .is_some_and(|s| s.len() as u64 > MAX_SOURCE_BYTES)
    {
        return Err(invalid("componentTsx exceeds maxSourceBytes (2097152)."));
    }
    let ui = project_context::run("ui", input.project_root.as_deref(), None).await?;
    let api = project_context::run("api", input.project_root.as_deref(), None).await?;
    let db = project_context::run("db", input.project_root.as_deref(), None).await?;
    let root = PathBuf::from(text(&ui["projectRoot"]));
    if root.as_os_str().is_empty() {
        return Ok(bound(
            json!({"status":"UNVERIFIED","message":"Project root could not be resolved","evidence":ui}),
            max_items,
        ));
    }
    let mut diagnostics = vec![];
    let operations = operations(&root, &api, &mut diagnostics);
    let handlers = handlers(&root, &mut diagnostics);
    let design = design(&input, artifacts).await;
    let mut chain = vec![];
    let mut selected_files = BTreeSet::new();
    let mut selected_ops = BTreeSet::new();
    let routes = array(&ui["routes"])
        .iter()
        .filter(|r| input.route_path.as_deref() == r["route"].as_str())
        .collect::<Vec<_>>();
    for route in &routes {
        selected_files.insert(text(&route["page"]).to_owned());
        chain.push(hop(
            "screen-component",
            route["route"].clone(),
            route["page"].clone(),
            json!({"declaration":"App Router page file","inventory":route}),
            None,
        ));
    }
    if supplied(&input.route_path) && routes.is_empty() {
        chain.push(hop("screen-component",json!(input.route_path),Value::Null,json!({}),Some("No matching route in the bounded UI inventory; no API path inferred from the screen path.")));
    }
    for c in array(&ui["components"]) {
        let direct = input.component_path.as_deref() == c["path"].as_str();
        let sites = array(&c["importedBy"])
            .iter()
            .filter(|site| {
                array(&site["routePages"])
                    .iter()
                    .any(|p| routes.iter().any(|r| r["page"] == *p))
            })
            .collect::<Vec<_>>();
        if direct || !sites.is_empty() {
            selected_files.insert(text(&c["path"]).into());
            if !sites.is_empty() {
                chain.push(hop("screen-component",json!(input.route_path),c["path"].clone(),json!({"importSites":sites,"meaning":"Static import reachability, not proof of rendering"}),None));
            }
        }
    }
    if supplied(&input.component_path)
        && !selected_files.contains(input.component_path.as_deref().unwrap())
    {
        chain.push(hop(
            "screen-component",
            json!(input.component_path),
            Value::Null,
            json!({}),
            Some("Component path is absent from the bounded authoritative UI inventory."),
        ));
    }
    for (i, op) in operations.iter().enumerate() {
        let id = supplied(&input.operation_id)
            && input.operation_id.as_deref() == op.endpoint["operationId"].as_str();
        let path = supplied(&input.api_path)
            && input.api_path.as_deref() == op.endpoint["path"].as_str()
            && input
                .method
                .as_deref()
                .is_some_and(|m| m.eq_ignore_ascii_case(text(&op.endpoint["method"])));
        if id || path {
            selected_ops.insert(i);
        }
    }
    if (supplied(&input.operation_id) || supplied(&input.api_path)) && selected_ops.is_empty() {
        chain.push(hop("component-api",json!({"operationId":input.operation_id,"apiPath":input.api_path,"method":input.method}),Value::Null,json!({}),Some("Explicit operation anchor was not found in project OpenAPI endpoints.")));
    }
    for file in &selected_files {
        let source = match read(&root.join(file)) {
            Ok(s) => s,
            Err(reason) => {
                chain.push(hop(
                    "component-api",
                    json!(file),
                    Value::Null,
                    json!({}),
                    Some(&reason),
                ));
                continue;
            }
        };
        let (calls, configs) = stack_diff::parse::client_references(&source);
        if calls.is_empty() && configs.is_empty() {
            chain.push(hop("component-api",json!(file),Value::Null,json!({}),Some("No supported literal API reference in this file; helpers, wrappers and dynamic calls are not resolved.")));
        }
        for (call_site, identifier) in calls {
            let matches = operations
                .iter()
                .enumerate()
                .filter(|(_, op)| {
                    op.endpoint["operationId"] == identifier
                        || (identifier.starts_with('/')
                            && stack_diff::client_path_key(&identifier)
                                == stack_diff::client_path_key(text(&op.endpoint["path"])))
                })
                .collect::<Vec<_>>();
            if matches.len() == 1 {
                let (i, op) = matches[0];
                selected_ops.insert(i);
                chain.push(hop("component-api",json!(file),op.endpoint.clone(),json!({"callSite":call_site,"identifier":identifier,"specPath":op.file,"parser":"stack_diff::client_references","limitation":"Parser retains identifier but not all call methods; no runtime or method compatibility proof"}),None));
            } else {
                chain.push(hop("component-api",json!(file),json!(identifier),json!({"callSite":call_site,"candidateCount":matches.len()}),Some("API identifier is absent or ambiguous across methods/spec authorities; no operation guessed.")));
            }
        }
        for (call_site, name) in configs {
            let matches = operations
                .iter()
                .enumerate()
                .filter(|(_, op)| {
                    array(&op.operation["tags"]).iter().any(|tag| {
                        ["one", "create", "edit", "fix"]
                            .iter()
                            .any(|kind| *tag == format!("devup:{name}:{kind}"))
                    })
                })
                .collect::<Vec<_>>();
            if matches.is_empty() {
                chain.push(hop(
                    "component-api",
                    json!(file),
                    json!(name),
                    json!({"callSite":call_site}),
                    Some("CRUD configuration has no matching devup:NAME:one/create/edit/fix tags."),
                ));
            }
            for (i, op) in matches {
                selected_ops.insert(i);
                chain.push(hop("component-api",json!(file),op.endpoint.clone(),json!({"callSite":call_site,"config":name,"tags":op.operation["tags"],"specPath":op.file,"meaning":"CRUD configuration tag membership; no particular action inferred"}),None));
            }
        }
    }
    // A table anchor can travel backwards only through explicit local references.
    if let Some(table) = input.table_name.as_deref().filter(|s| !s.trim().is_empty()) {
        for (i, op) in operations.iter().enumerate() {
            if handlers.iter().any(|h| {
                handler_matches(h, op, &root)
                    && array(&db["tables"])
                        .iter()
                        .filter(|t| t["table"] == table)
                        .any(|t| {
                            array(&t["columns"]).iter().any(|c| {
                                stack_diff::parse::route_mapping(&h.source)
                                    .mentions(table, text(&c["name"]))
                            })
                        })
            }) {
                selected_ops.insert(i);
            }
        }
    }
    for &i in &selected_ops {
        let op = &operations[i];
        let matches = handlers
            .iter()
            .filter(|h| handler_matches(h, op, &root))
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            chain.push(hop("api-handler",op.endpoint.clone(),Value::Null,json!({"specPath":op.file,"candidateCount":matches.len()}),Some("No unique local handler in the spec authority; merged prefixes, macro expansion and reexports are not resolved.")));
            continue;
        }
        let h = matches[0];
        let file = relative(&root, &h.file);
        chain.push(hop("api-handler",op.endpoint.clone(),json!(file),json!({"attributeMethod":h.method,"fileConventionPath":h.path,"specPath":op.file,"parser":"stack_diff route attribute and path comparison","limitation":"File convention is heuristic; merged-app runtime registration is not proven"}),None));
        let mapping = stack_diff::parse::route_mapping(&h.source);
        let mut found = false;
        for t in array(&db["tables"]) {
            let table = text(&t["table"]);
            // Keep model authority local to the spec/handler project.
            if Path::new(text(&t["path"])).parent().and_then(Path::parent)
                != Path::new(&op.file).parent()
            {
                continue;
            }
            let columns = array(&t["columns"])
                .iter()
                .filter(|c| mapping.mentions(table, text(&c["name"])))
                .map(|c| c["name"].clone())
                .collect::<Vec<_>>();
            if columns.is_empty() {
                continue;
            }
            found = true;
            let reason=(handlers.iter().filter(|other|other.file==h.file).count()>1).then_some("Multiple handlers share this file; file-level schema references cannot be attributed to this handler.");
            chain.push(hop("handler-model",json!(file),json!(table),json!({"modelPath":t["path"],"parser":"stack_diff::route_mapping","meaning":"Explicit file-level model/schema_type pick/omit references; serialization and type alias usage not proven"}),reason));
            chain.push(hop("model-columns",json!(table),json!(columns),json!({"modelPath":t["path"],"routeFile":file,"columns":columns,"mappingUnresolved":mapping.unresolved}),reason));
        }
        if !found {
            chain.push(hop("handler-model",json!(file),Value::Null,json!({}),Some("No explicit local model/column mapping resolved; substring guesses are not used.")));
        }
    }
    if let Some(table) = input.table_name.as_deref().filter(|s| !s.trim().is_empty()) {
        let tables = array(&db["tables"])
            .iter()
            .filter(|t| t["table"] == table)
            .collect::<Vec<_>>();
        if tables.len() == 1 {
            chain.push(hop("model-columns",json!(table),tables[0]["columns"].clone(),json!({"modelPath":tables[0]["path"],"declaration":"Vespertide model JSON columns"}),None));
        } else {
            chain.push(hop(
                "model-columns",
                json!(table),
                Value::Null,
                json!({"candidateCount":tables.len()}),
                Some("Table anchor is absent or ambiguous in the bounded model inventory."),
            ));
        }
    }
    if supplied(&input.figma_node_id) || supplied(&input.artifact_id) {
        chain.push(hop("design-component",json!({"figmaNodeId":input.figma_node_id,"artifactId":input.artifact_id}),json!(design.names),design.evidence.clone(),Some("Design composition names identify reuse candidates, not an established design-to-code binding.")));
    }
    for kind in [
        "screen-component",
        "component-api",
        "api-handler",
        "handler-model",
        "model-columns",
    ] {
        if !chain.iter().any(|h| h["kind"] == kind) {
            chain.push(hop(
                kind,
                Value::Null,
                Value::Null,
                json!({}),
                Some("No evidence joins this hop to the supplied anchors; no anchor was inferred."),
            ));
        }
    }
    let mut reuse = vec![];
    for c in array(&ui["components"]) {
        let mut reasons = vec![];
        let mut score = 0;
        for name in array(&c["exports"]).iter().chain(array(&c["localNames"])) {
            if design.names.contains(text(name)) {
                reasons.push(format!(
                    "Export/local name {} exactly matches a design JSX reference",
                    text(name)
                ));
                score += 100;
            }
        }
        for prop in array(&c["props"]) {
            if design.fields.contains(text(&prop["name"])) {
                reasons.push(format!(
                    "Declared prop {} matches a literal design field binding",
                    text(&prop["name"])
                ));
                score += 10;
            }
        }
        if selected_files.contains(text(&c["path"])) {
            reasons.push("Already belongs to the explicitly selected component/route slice".into());
            score += 20;
        }
        if score > 0 {
            reuse.push(json!({"component":c,"score":score,"matchReasons":reasons,"confidence":"medium","limitation":"Name/prop compatibility is syntactic; visual equivalence and runtime behavior are unverified"}));
        }
    }
    reuse.sort_by(|a, b| {
        b["score"]
            .as_u64()
            .cmp(&a["score"].as_u64())
            .then_with(|| text(&a["component"]["path"]).cmp(text(&b["component"]["path"])))
    });
    for (i, c) in reuse.iter_mut().enumerate() {
        c["rank"] = json!(i + 1);
    }
    let comparisons = if (supplied(&input.figma_node_id) || supplied(&input.artifact_id))
        && (supplied(&input.operation_id) || supplied(&input.api_path))
    {
        selected_ops
            .iter()
            .filter(|&&i| {
                (supplied(&input.operation_id)
                    && input.operation_id.as_deref()
                        == operations[i].endpoint["operationId"].as_str())
                    || (supplied(&input.api_path)
                        && input.api_path.as_deref() == operations[i].endpoint["path"].as_str()
                        && input.method.as_deref().is_some_and(|m| {
                            m.eq_ignore_ascii_case(text(&operations[i].endpoint["method"]))
                        }))
            })
            .map(|&i| compare(&design, &operations[i]))
            .collect::<Vec<_>>()
    } else {
        vec![]
    };
    let mut ownership = vec![];
    for layer in [
        "db-entity",
        "entity-route",
        "route-openapi",
        "openapi-client",
    ] {
        let mut result = json!({"selfCheck":{}});
        stack_diff::attach_source_ownership(&mut result, layer);
        ownership.push(result["selfCheck"]["sourceOwnership"].clone());
    }
    let mut traced_artifacts = BTreeMap::new();
    for file in &selected_files {
        traced_artifacts.insert(file.clone(),json!({"path":file,"ownership":"unverified","editTarget":null,
            "reason":"UI inventory identifies source declarations but does not certify that a local file is human-authored. Check generated-code provenance before editing.","sourceOwnership":ownership[3]}));
    }
    for &i in &selected_ops {
        let operation = &operations[i];
        let matches = handlers
            .iter()
            .filter(|h| handler_matches(h, operation, &root))
            .collect::<Vec<_>>();
        let edit_target = (matches.len() == 1)
            .then(|| relative(&root, &matches[0].file))
            .filter(|path| !generated_path(path, &ownership[2]));
        traced_artifacts.insert(operation.file.clone(),json!({"path":operation.file,"ownership":"generated","editTarget":edit_target,"sourceOwnership":ownership[2]}));
        for handler in matches {
            let file = relative(&root, &handler.file);
            traced_artifacts.insert(file.clone(), authored_artifact(&file, &ownership[1]));
        }
    }
    for hop in &chain {
        if let Some(path) = hop["evidence"]["modelPath"].as_str() {
            traced_artifacts.insert(path.into(), authored_artifact(path, &ownership[0]));
        }
    }
    let mut implemented = BTreeMap::<String, Vec<Value>>::new();
    for file in &selected_files {
        if let Ok(source) = read(&root.join(file)) {
            for state in design_source(&source).states {
                implemented.entry(state).or_default().push(json!({"path":file,"declaration":"literal data-state JSX attribute; runtime reachability remains unverified"}));
            }
        }
    }
    for route in &routes {
        let page = Path::new(text(&route["page"]));
        let owner = if route["appliesTo"] == "." {
            PathBuf::new()
        } else {
            PathBuf::from(text(&route["appliesTo"]))
        };
        let app = if page.starts_with(owner.join("src/app")) {
            owner.join("src/app")
        } else {
            owner.join("app")
        };
        for component in array(&ui["components"]) {
            let boundary = Path::new(text(&component["path"]));
            let state = boundary.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if matches!(state, "loading" | "error")
                && boundary.starts_with(&app)
                && boundary.parent().is_some_and(|p| page.starts_with(p))
                && component["appliesTo"] == route["appliesTo"]
            {
                implemented.entry(state.into()).or_default().push(json!({"path":component["path"],"declaration":"App Router boundary component in selected page ancestry; runtime appearance unverified"}));
            }
        }
    }
    let states=["loading","error","empty","validation","authorization"].iter().map(|state|json!({"state":state,
        "specified":if design.states.contains(*state){"present"}else{"unknown"},
        "implemented":if implemented.contains_key(*state){"present"}else{"unknown"},
        "specifiedEvidence":if design.states.contains(*state){json!({"attribute":"data-state","value":state,"source":design.evidence})}else{Value::Null},
        "implementedEvidence":implemented.get(*state),
        "reason":"Presence records explicit state declarations or App Router boundary components only. Missing evidence is unknown, never assumed absent; runtime coverage requires tests."})).collect::<Vec<_>>();
    let acceptance=input.acceptance_criteria.iter().map(|criterion|json!({"criterion":criterion,"status":"UNVERIFIED","reason":"Caller acceptance text is preserved without semantic interpretation; supply tests or inspect the linked evidence."})).collect::<Vec<_>>();
    Ok(bound(
        json!({"status":"OK","projectRoot":root,"anchors":{"routePath":input.route_path,"figmaNodeId":input.figma_node_id,"artifactId":input.artifact_id,"operationId":input.operation_id,"apiPath":input.api_path,"method":input.method,"componentPath":input.component_path,"tableName":input.table_name},"requirement":input.requirement,"acceptanceMatrix":acceptance,
        "chain":chain,"artifacts":traced_artifacts.into_values().collect::<Vec<_>>(),"sourceOwnership":ownership,"reuseCandidates":reuse,"designContract":comparisons,"requiredStates":states,
        "designEvidence":design.evidence,"designDiagnostics":design.diagnostics,"diagnostics":diagnostics,
        "inventoryEvidence":{"uiTruncation":ui["truncation"],"uiLimits":ui["limits"],"uiDiagnostics":ui["diagnostics"],"uiExcludedPaths":ui["excludedPaths"],"unparsedFiles":ui["unparsedFiles"],"api":{"found":api["found"],"excludedPaths":api["excludedPaths"],"authorityNote":api["authorityNote"],"issues":array(&api["specs"]).iter().filter(|v|v.get("parseError").is_some()||v.get("readError").is_some()).collect::<Vec<_>>()},
            "db":{"found":db["found"],"excludedPaths":db["excludedPaths"],"authorityNote":db["authorityNote"],"issues":array(&db["tables"]).iter().filter(|v|v.get("parseError").is_some()||v.get("readError").is_some()).collect::<Vec<_>>()}
},
        "limitations":["Read-only static evidence, never runtime execution proof. Shared parser findings retain low/medium confidence.","Missing results do not prove absence: scan depth, UI inventory caps, unreadable files, dynamic imports/calls, wrappers and macro expansion can hide links.","Source ownership comes unchanged from stack_diff; edit authored models/routes/frontend and regenerate generated artifacts."]}),
        max_items,
    ))
}
fn handler_matches(handler: &Handler, operation: &Operation, root: &Path) -> bool {
    let authority = root.join(&operation.file).parent().unwrap().to_path_buf();
    handler.file.starts_with(authority.join("src/routes"))
        && stack_diff::route_comparison_key(&handler.method, &handler.path)
            == stack_diff::route_comparison_key(
                text(&operation.endpoint["method"]),
                text(&operation.endpoint["path"]),
            )
}

#[cfg(test)]
#[path = "feature_trace_tests.rs"]
mod tests;
