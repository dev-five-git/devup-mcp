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
const ACCEPTED: &str = include_str!("../text/feature_trace-ACCEPTED-59546b50.md");

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
            if artifact.capabilities.kind == super::artifacts::ArtifactKind::Measurement {
                return Design {
                    evidence: json!({"source":"measurement-only-artifact","artifactId":artifact.artifact_id,"state":"unverified","capabilities":artifact.capabilities}),
                    diagnostics: vec![
                        json!({"reason":"Measurement receipts contain logical dimensions only; design structure, component bindings and code were not acquired. Export a design snapshot for feature tracing."}),
                    ],
                    ..Design::default()
                };
            }
            let node = input.figma_node_id.as_deref().or(artifact
                .payload
                .target
                .node_id
                .as_deref());
            if let Some(node) = node {
                let interaction_candidates = serde_json::to_value(super::interactions::candidates(
                    &artifact.payload.snapshot,
                    &[node.to_owned()],
                ))
                .unwrap();
                match generate_component(
                    &artifact.payload.snapshot,
                    node,
                    &CodegenOptions::default().with_payload_tokens(&artifact.payload),
                ) {
                    Ok(output) => {
                        let mut design = design_source(&output.tsx);
                        design.evidence = json!({"source":"cached-artifact","artifactId":artifact.artifact_id,"nodeId":node,"sourceMap":output.source_map,"designFingerprints":design_fingerprints(&artifact.payload.snapshot),"staleness":"unknown: cached snapshot has not been compared with live Figma"});
                        design.evidence["interactiveCandidates"] = interaction_candidates;
                        return design;
                    }
                    Err(e) => {
                        return Design {
                            evidence: json!({"source":"cached-artifact","interactiveCandidates":interaction_candidates}),
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
        // oxc identifies actual comments so string literals cannot spoof hints.
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
        let hints = if parsed.diagnostics.is_empty() && !parsed.panicked {
            parsed
                .program
                .comments
                .iter()
                .filter_map(|c| {
                    c.span
                        .source_text(source)
                        .strip_prefix("// @devup-interaction ")
                })
                .filter_map(|s| serde_json::from_str::<Value>(s).ok())
                .filter(|v| v["nodeId"].is_string() && v["prototypeActions"].is_array())
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        design.evidence["interactiveCandidates"] = json!(hints);
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
        "scope":include_str!("../text/feature_trace-comparison-60526fe1.md")})
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
            .filter(|(k, _)| !matches!(k.as_str(), "truncation" | "limits" | "status" | "summary"))
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