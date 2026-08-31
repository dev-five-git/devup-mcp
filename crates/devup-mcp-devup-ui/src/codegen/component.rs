use std::collections::{BTreeMap, BTreeSet, HashSet};

use devup_mcp_figma::{
    CollectedPayload, DevupError, Diagnostic, ErrorCode, RawNode, Snapshot, UpstreamResult,
};

use super::{layout, style, text, variant};

#[derive(Debug, Clone, Default)]
pub struct CodegenOptions {
    pub component_name: Option<String>,
    pub include_diagnostics: bool,
    pub inline_instances: bool,
    pub text_style_tokens: std::collections::BTreeMap<String, String>,
    pub variable_tokens: std::collections::BTreeMap<String, String>,
}

impl CodegenOptions {
    pub fn with_payload_tokens(mut self, payload: &CollectedPayload) -> Self {
        self.text_style_tokens = named_tokens(payload.styles.as_ref(), "styles");
        self.variable_tokens = named_tokens(payload.variables.as_ref(), "variables");
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenOutput {
    pub tsx: String,
    pub imports: Vec<String>,
    pub used_tokens: BTreeSet<String>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn generate_component(
    snapshot: &Snapshot,
    root_id: &str,
    options: &CodegenOptions,
) -> Result<CodegenOutput, DevupError> {
    let generated = generate_node(snapshot, root_id, options)?;
    let root = snapshot.nodes.get(root_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Figma snapshot에서 변환할 node를 찾지 못했습니다.",
            false,
        )
    })?;
    let component_name = options
        .component_name
        .as_deref()
        .map(normalize_component_name)
        .unwrap_or_else(|| normalize_component_name(root.typed_view().name().unwrap_or("")));
    let body = generated
        .tsx
        .lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut tsx = format!(
        "import {{ {} }} from \"@devup-ui/react\";\n\n",
        generated.imports.join(", ")
    );
    tsx.push_str(&format!(
        "export function {component_name}() {{\n  return (\n{body}\n  );\n}}\n"
    ));
    Ok(CodegenOutput { tsx, ..generated })
}

pub fn generate_legacy_component(
    snapshot: &Snapshot,
    root_id: &str,
    options: &CodegenOptions,
) -> Result<CodegenOutput, DevupError> {
    let generated = generate_node(snapshot, root_id, options)?;
    let root = snapshot.nodes.get(root_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Figma snapshot에서 변환할 node를 찾지 못했습니다.",
            false,
        )
    })?;
    let component_name = options
        .component_name
        .as_deref()
        .map(legacy_component_name)
        .unwrap_or_else(|| legacy_component_name(root.typed_view().name().unwrap_or("")));
    let body = if generated.tsx.contains('\n') {
        let indented = generated
            .tsx
            .lines()
            .map(|line| format!("    {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("(\n{indented}\n  )")
    } else {
        generated.tsx.clone()
    };
    Ok(CodegenOutput {
        tsx: format!("export function {component_name}() {{\n  return {body}\n}}"),
        ..generated
    })
}

pub fn render_component_source(
    component: &str,
    code: &str,
    variants: &[(String, String)],
) -> String {
    let variants = variants
        .iter()
        .filter(|(key, _)| !key.eq_ignore_ascii_case("effect"))
        .collect::<Vec<_>>();
    let wrapped = if code.contains('\n') {
        format!(
            "(\n{}\n  )",
            code.lines()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    } else {
        code.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    if variants.is_empty() {
        return format!("export function {component}() {{\n  return {wrapped}\n}}");
    }
    let interface = variants
        .iter()
        .map(|(key, value)| {
            let optional = if value == "boolean" { "?" } else { "" };
            format!("  {key}{optional}: {value}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let keys = variants
        .iter()
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "export interface {component}Props {{\n{interface}\n}}\n\nexport function {component}({{ {keys} }}: {component}Props) {{\n  return {wrapped}\n}}"
    )
}

pub fn generate_component_set_target(
    snapshot: &Snapshot,
    root_id: &str,
    target_name: &str,
    options: &CodegenOptions,
) -> Result<CodegenOutput, DevupError> {
    let root = snapshot.nodes.get(root_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Figma snapshot에서 component set을 찾지 못했습니다.",
            false,
        )
    })?;
    let component_set = if root.typed_view().node_type() == "COMPONENT_SET"
        && root.typed_view().name() == Some(target_name)
    {
        Some(root)
    } else {
        snapshot.nodes.values().find(|node| {
            node.typed_view().node_type() == "COMPONENT_SET"
                && node.typed_view().name() == Some(target_name)
        })
    };
    let root_view = component_set.unwrap_or(root).typed_view();
    let main = component_set.is_some();
    if let Some(set) = component_set
        && let Some(output) = variant::generate_variant_component_set(snapshot, &set.id, options)?
    {
        return Ok(output);
    }
    let target_id = if main {
        let default_name = root_view
            .value("defaultVariant")
            .and_then(|value| value.get("name"))
            .and_then(serde_json::Value::as_str);
        root_view
            .child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .find(|node| node.typed_view().name() == default_name)
            .or_else(|| {
                root_view
                    .child_ids()
                    .filter_map(|id| snapshot.nodes.get(id))
                    .next()
            })
            .map(|node| node.id.as_str())
    } else {
        snapshot
            .nodes
            .values()
            .find(|node| {
                node.typed_view().node_type() == "COMPONENT"
                    && node.typed_view().name() == Some(target_name)
            })
            .map(|node| node.id.as_str())
    }
    .ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            format!("component set에서 '{target_name}' 출력을 찾지 못했습니다."),
            false,
        )
    })?;

    let generated = if main {
        match generate_component_asset_child(snapshot, target_id, options) {
            Some(output) => output?,
            None => generate_node(snapshot, target_id, options)?,
        }
    } else {
        generate_node(snapshot, target_id, options)?
    };
    let code = if !main && generated.tsx.trim() == "<Box />" {
        generated.tsx.replace("<Box />", "<Box boxSize=\"100%\" />")
    } else {
        generated.tsx.clone()
    };
    let variants = if main {
        component_variants(
            snapshot,
            component_set.expect("main component set"),
            target_id,
        )
    } else {
        Vec::new()
    };
    Ok(CodegenOutput {
        tsx: render_component_source(&legacy_component_name(target_name), &code, &variants),
        ..generated
    })
}

pub fn generate_inlined_component_instance(
    snapshot: &Snapshot,
    root_id: &str,
    instance_id: &str,
    options: &CodegenOptions,
) -> Result<CodegenOutput, DevupError> {
    let root = snapshot.nodes.get(root_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "inline instance root를 찾지 못했습니다.",
            false,
        )
    })?;
    let instance = snapshot.nodes.get(instance_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "inline할 component instance를 찾지 못했습니다.",
            false,
        )
    })?;
    let name = instance.typed_view().name().unwrap_or("Component");
    let selected_variants = instance
        .typed_view()
        .value("componentProperties")
        .and_then(serde_json::Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .filter_map(|(key, value)| {
                    (value.get("type").and_then(serde_json::Value::as_str) == Some("VARIANT"))
                        .then(|| value.get("value").and_then(serde_json::Value::as_str))
                        .flatten()
                        .map(|value| (key.as_str(), value))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let set = snapshot
        .nodes
        .values()
        .find(|node| {
            node.typed_view().node_type() == "COMPONENT_SET"
                && node.typed_view().name() == Some(name)
        })
        .ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                format!("'{name}' component set을 찾지 못했습니다."),
                false,
            )
        })?;
    let selected = set
        .typed_view()
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .find(|candidate| {
            selected_variants.iter().all(|(key, value)| {
                candidate
                    .typed_view()
                    .value("variantProperties")
                    .and_then(|variants| variants.get(*key))
                    .and_then(serde_json::Value::as_str)
                    == Some(*value)
            })
        })
        .ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                format!("'{name}' instance variant를 찾지 못했습니다."),
                false,
            )
        })?;
    let mut projected = snapshot.clone();
    if let Some(projected_set) = projected.nodes.get_mut(&set.id)
        && let Some(children) = projected_set
            .fields
            .get_mut("childrenIds")
            .and_then(serde_json::Value::as_array_mut)
    {
        children.retain(|id| id.as_str() != Some(&selected.id));
    }
    if let Some(projected_selected) = projected.nodes.get_mut(&selected.id) {
        for field in ["layoutSizingHorizontal", "layoutSizingVertical"] {
            projected_selected.fields.insert(
                field.to_owned(),
                serde_json::Value::String("FIXED".to_owned()),
            );
        }
    }
    let mut output = generate_node(&projected, &selected.id, options)?;
    if let Some(close) = output.tsx.rfind("\n/>") {
        let mut lines = output.tsx[..close]
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for (field, prop) in [("height", "h"), ("width", "w")] {
            if let Some(value) = root.typed_view().number(field) {
                lines.push(format!("  {prop}=\"{}\"", layout::px(value)));
            }
        }
        lines[1..].sort();
        output.tsx = format!("{}\n/>", lines.join("\n"));
    }
    let usage = selected_variants
        .iter()
        .map(|(key, value)| format!(" {key}=\"{value}\""))
        .collect::<String>();
    output.tsx = format!("{{/* <{name}{usage} /> */}}\n{}", output.tsx);
    Ok(output)
}

fn generate_component_asset_child(
    snapshot: &Snapshot,
    component_id: &str,
    options: &CodegenOptions,
) -> Option<Result<CodegenOutput, DevupError>> {
    let component = snapshot.nodes.get(component_id)?;
    let children = component.typed_view().child_ids().collect::<Vec<_>>();
    if children.len() != 1 {
        return None;
    }
    let child_id = children[0];
    let child = snapshot.nodes.get(child_id)?;
    style::asset_kind(snapshot, child)?;
    let mut projected = snapshot.clone();
    let projected_child = projected.nodes.get_mut(child_id)?;
    for dimension in ["width", "height"] {
        if projected_child.typed_view().number(dimension).is_none()
            && let Some(value) = component.typed_view().number(dimension)
        {
            projected_child
                .fields
                .insert(dimension.to_owned(), serde_json::Value::from(value));
        }
    }
    projected_child.fields.insert(
        "layoutSizingHorizontal".to_owned(),
        serde_json::Value::String("FIXED".to_owned()),
    );
    projected_child.fields.insert(
        "layoutSizingVertical".to_owned(),
        serde_json::Value::String("FIXED".to_owned()),
    );
    Some(generate_node(&projected, child_id, options))
}

pub fn render_component_registration_snapshot(
    snapshot: &Snapshot,
    root_id: &str,
    target_name: &str,
) -> Result<String, DevupError> {
    let root = snapshot.nodes.get(root_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "component registration root를 찾지 못했습니다.",
            false,
        )
    })?;
    let set = if root.typed_view().node_type() == "COMPONENT_SET" {
        Some(root)
    } else {
        snapshot
            .nodes
            .values()
            .find(|node| node.typed_view().node_type() == "COMPONENT_SET")
    };
    let selected = if root.typed_view().node_type() == "COMPONENT" {
        root
    } else {
        root.typed_view()
            .child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .find(|node| node.typed_view().name() == Some(target_name))
            .ok_or_else(|| {
                DevupError::new(
                    ErrorCode::DevupFigmaNodeNotFound,
                    format!("registration 대상 '{target_name}'을 찾지 못했습니다."),
                    false,
                )
            })?
    };
    let name = set
        .and_then(|set| set.typed_view().name())
        .or_else(|| selected.typed_view().name())
        .unwrap_or("Component");
    let selected_is_set_child = set.is_some_and(|set| {
        set.typed_view()
            .child_ids()
            .any(|child| child == selected.id)
    });
    let node = registration_node(snapshot, selected, set, selected_is_set_child, 1);
    let variants = set
        .map(|set| component_variants(snapshot, set, &selected.id))
        .unwrap_or_default()
        .into_iter()
        .filter(|(key, _)| !key.eq_ignore_ascii_case("effect"))
        .collect::<Vec<_>>();
    let variants = if variants.is_empty() {
        "{}".to_owned()
    } else {
        let lines = variants
            .iter()
            .map(|(key, value)| format!("    {}: {},", json_string(key), json_string(value)))
            .collect::<Vec<_>>()
            .join("\n");
        format!("{{\n{lines}\n  }}")
    };
    let box_size = set.is_none();
    let mut props = Vec::new();
    if let Some(set) = set
        && selected
            .typed_view()
            .value("reactions")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|reactions| {
                reactions.iter().any(|reaction| {
                    reaction
                        .get("trigger")
                        .and_then(|trigger| trigger.get("type"))
                        .and_then(serde_json::Value::as_str)
                        == Some("ON_HOVER")
                })
            })
        && let Some(hover) = set
            .typed_view()
            .child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .find(|node| node.id != selected.id && node.typed_view().number("opacity").is_some())
        && let Some(opacity) = hover.typed_view().number("opacity")
    {
        props.extend([
            "      \"_hover\": {".to_owned(),
            format!(
                "        \"opacity\": {},",
                json_string(&layout::format_number(opacity))
            ),
            "      },".to_owned(),
            "      \"aspectRatio\": undefined,".to_owned(),
            "      \"flex\": undefined,".to_owned(),
            "      \"h\": undefined,".to_owned(),
            "      \"maxH\": undefined,".to_owned(),
            "      \"maxW\": undefined,".to_owned(),
            "      \"minH\": undefined,".to_owned(),
            "      \"minW\": undefined,".to_owned(),
            "      \"transition\": \"0.3ms ease-in-out\",".to_owned(),
            "      \"transitionProperty\": \"opacity\",".to_owned(),
            "      \"w\": undefined,".to_owned(),
        ]);
    } else {
        props.push("      \"aspectRatio\": undefined,".to_owned());
    }
    if !props.iter().any(|prop| prop.contains("\"_hover\"")) {
        if box_size {
            props.push("      \"boxSize\": \"100%\",".to_owned());
        }
        props.push("      \"flex\": undefined,".to_owned());
        if !box_size {
            props.push("      \"h\": undefined,".to_owned());
        }
        props.extend([
            "      \"maxH\": undefined,".to_owned(),
            "      \"maxW\": undefined,".to_owned(),
            "      \"minH\": undefined,".to_owned(),
            "      \"minW\": undefined,".to_owned(),
        ]);
        if !box_size {
            props.push("      \"w\": undefined,".to_owned());
        }
    }
    Ok(format!(
        "{{\n  \"name\": {},\n  \"node\": {},\n  \"tree\": {{\n    \"children\": [],\n    \"component\": \"Box\",\n    \"nodeName\": {},\n    \"nodeType\": \"COMPONENT\",\n    \"props\": {{\n{}\n    }},\n  }},\n  \"variantComments\": {{}},\n  \"variants\": {},\n}}",
        json_string(name),
        node,
        json_string(selected.typed_view().name().unwrap_or(target_name)),
        props.join("\n"),
        variants
    ))
}

fn registration_node(
    snapshot: &Snapshot,
    node: &RawNode,
    parent: Option<&RawNode>,
    circular_children: bool,
    depth: usize,
) -> String {
    let view = node.typed_view();
    let indent = "  ".repeat(depth);
    let child_indent = "  ".repeat(depth + 1);
    let mut entries = Vec::new();
    entries.push(format!("{child_indent}\"children\": [],"));
    entries.push(format!(
        "{child_indent}\"name\": {},",
        json_string(view.name().unwrap_or(""))
    ));
    if let Some(opacity) = view.number("opacity") {
        entries.push(format!(
            "{child_indent}\"opacity\": {},",
            layout::format_number(opacity)
        ));
    }
    if let Some(parent) = parent {
        entries.push(format!(
            "{child_indent}\"parent\": {},",
            registration_parent(snapshot, parent, node, circular_children, depth + 1)
        ));
    }
    if let Some(value) = view.value("reactions") {
        entries.push(format!(
            "{child_indent}\"reactions\": {},",
            js_value(value, depth + 1)
        ));
    }
    entries.push(format!(
        "{child_indent}\"type\": {},",
        json_string(view.node_type())
    ));
    if let Some(value) = view.value("variantProperties") {
        entries.push(format!(
            "{child_indent}\"variantProperties\": {},",
            js_value(value, depth + 1)
        ));
    }
    if let Some(visible) = view.bool("visible") {
        entries.push(format!("{child_indent}\"visible\": {visible},"));
    }
    format!("{{\n{}\n{indent}}}", entries.join("\n"))
}

fn registration_parent(
    snapshot: &Snapshot,
    parent: &RawNode,
    selected: &RawNode,
    circular_children: bool,
    depth: usize,
) -> String {
    let view = parent.typed_view();
    let indent = "  ".repeat(depth);
    let child_indent = "  ".repeat(depth + 1);
    let children_rendered = if view.child_ids().next().is_none() {
        "[]".to_owned()
    } else {
        let values = view
            .child_ids()
            .filter_map(|id| {
                if id == selected.id {
                    Some("[Circular]".to_owned())
                } else {
                    snapshot
                        .nodes
                        .get(id)
                        .map(|child| registration_child(child, circular_children, depth + 2))
                }
            })
            .collect::<Vec<_>>();
        if values.is_empty() {
            "[]".to_owned()
        } else {
            format!(
                "[\n{}\n{child_indent}]",
                values
                    .iter()
                    .map(|value| format!("{}{},", "  ".repeat(depth + 2), value))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        }
    };
    let mut entries = vec![format!("{child_indent}\"children\": {children_rendered},")];
    for field in ["componentPropertyDefinitions", "defaultVariant"] {
        if let Some(value) = view.value(field) {
            let rendered = if field == "defaultVariant"
                && value.get("name").and_then(serde_json::Value::as_str)
                    == selected.typed_view().name()
                && circular_children
            {
                "[Circular]".to_owned()
            } else if field == "defaultVariant" {
                registration_embedded_node(value, depth + 1)
            } else {
                js_value(value, depth + 1)
            };
            entries.push(format!("{child_indent}{}: {rendered},", json_string(field)));
        }
    }
    entries.push(format!(
        "{child_indent}\"name\": {},",
        json_string(view.name().unwrap_or(""))
    ));
    entries.push(format!(
        "{child_indent}\"type\": {},",
        json_string(view.node_type())
    ));
    if let Some(visible) = view.bool("visible") {
        entries.push(format!("{child_indent}\"visible\": {visible},"));
    }
    format!("{{\n{}\n{indent}}}", entries.join("\n"))
}

fn registration_child(node: &RawNode, circular_parent: bool, depth: usize) -> String {
    let view = node.typed_view();
    let indent = "  ".repeat(depth);
    let child_indent = "  ".repeat(depth + 1);
    let mut entries = vec![
        format!("{child_indent}\"children\": [],"),
        format!(
            "{child_indent}\"name\": {},",
            json_string(view.name().unwrap_or(""))
        ),
    ];
    if let Some(opacity) = view.number("opacity") {
        entries.push(format!(
            "{child_indent}\"opacity\": {},",
            layout::format_number(opacity)
        ));
    }
    if circular_parent {
        entries.push(format!("{child_indent}\"parent\": [Circular],"));
    }
    if let Some(value) = view.value("reactions") {
        entries.push(format!(
            "{child_indent}\"reactions\": {},",
            js_value(value, depth + 1)
        ));
    }
    entries.push(format!(
        "{child_indent}\"type\": {},",
        json_string(view.node_type())
    ));
    if let Some(value) = view.value("variantProperties") {
        entries.push(format!(
            "{child_indent}\"variantProperties\": {},",
            js_value(value, depth + 1)
        ));
    }
    if let Some(visible) = view.bool("visible") {
        entries.push(format!("{child_indent}\"visible\": {visible},"));
    }
    format!("{{\n{}\n{indent}}}", entries.join("\n"))
}

fn registration_embedded_node(value: &serde_json::Value, depth: usize) -> String {
    let Some(object) = value.as_object() else {
        return js_value(value, depth);
    };
    let indent = "  ".repeat(depth);
    let child_indent = "  ".repeat(depth + 1);
    let mut entries = vec![format!("{child_indent}\"children\": [],")];
    for field in [
        "name",
        "opacity",
        "reactions",
        "type",
        "variantProperties",
        "visible",
    ] {
        if let Some(value) = object.get(field) {
            entries.push(format!(
                "{child_indent}{}: {},",
                json_string(field),
                js_value(value, depth + 1)
            ));
        }
    }
    format!("{{\n{}\n{indent}}}", entries.join("\n"))
}

fn js_value(value: &serde_json::Value, depth: usize) -> String {
    match value {
        serde_json::Value::Null => "null".to_owned(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => json_string(value),
        serde_json::Value::Array(values) => {
            if values.is_empty() {
                "[]".to_owned()
            } else {
                let indent = "  ".repeat(depth);
                let child_indent = "  ".repeat(depth + 1);
                format!(
                    "[\n{}\n{indent}]",
                    values
                        .iter()
                        .map(|value| format!("{child_indent}{},", js_value(value, depth + 1)))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }
        }
        serde_json::Value::Object(values) => {
            if values.is_empty() {
                "{}".to_owned()
            } else {
                let indent = "  ".repeat(depth);
                let child_indent = "  ".repeat(depth + 1);
                format!(
                    "{{\n{}\n{indent}}}",
                    values
                        .iter()
                        .map(|(key, value)| format!(
                            "{child_indent}{}: {},",
                            json_string(key),
                            js_value(value, depth + 1)
                        ))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }
        }
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("string JSON serialization")
}

fn component_variants(
    snapshot: &Snapshot,
    root: &RawNode,
    default_id: &str,
) -> Vec<(String, String)> {
    let definitions = root
        .typed_view()
        .value("componentPropertyDefinitions")
        .and_then(serde_json::Value::as_object);
    let mut variants = Vec::new();
    let mut added = BTreeSet::new();
    let add = |raw_key: &str,
               definition: &serde_json::Value,
               variants: &mut Vec<(String, String)>,
               added: &mut BTreeSet<String>| {
        let key = component_property_name(raw_key);
        if added.contains(&key) {
            return;
        }
        let value = match definition.get("type").and_then(serde_json::Value::as_str) {
            Some("VARIANT") => definition
                .get("variantOptions")
                .and_then(serde_json::Value::as_array)
                .map(|options| {
                    options
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(|option| format!("'{option}'"))
                        .collect::<Vec<_>>()
                        .join(" | ")
                }),
            Some("BOOLEAN") => Some("boolean".to_owned()),
            Some("INSTANCE_SWAP") => Some("React.ReactNode".to_owned()),
            _ => None,
        };
        if let Some(value) = value {
            added.insert(key.clone());
            variants.push((key, value));
        }
    };
    if let Some(definitions) = definitions {
        for (key, definition) in definitions {
            if definition.get("type").and_then(serde_json::Value::as_str) == Some("VARIANT") {
                add(key, definition, &mut variants, &mut added);
            }
        }
        fn visit_refs(
            snapshot: &Snapshot,
            id: &str,
            definitions: &serde_json::Map<String, serde_json::Value>,
            variants: &mut Vec<(String, String)>,
            added: &mut BTreeSet<String>,
            add: &impl Fn(&str, &serde_json::Value, &mut Vec<(String, String)>, &mut BTreeSet<String>),
        ) {
            let Some(node) = snapshot.nodes.get(id) else {
                return;
            };
            let view = node.typed_view();
            if let Some(references) = view
                .value("componentPropertyReferences")
                .and_then(serde_json::Value::as_object)
            {
                for field in ["mainComponent", "visible"] {
                    if let Some(key) = references.get(field).and_then(serde_json::Value::as_str)
                        && let Some(definition) = definitions.get(key)
                    {
                        add(key, definition, variants, added);
                    }
                }
            }
            for child in view.child_ids() {
                visit_refs(snapshot, child, definitions, variants, added, add);
            }
        }
        visit_refs(
            snapshot,
            default_id,
            definitions,
            &mut variants,
            &mut added,
            &add,
        );
        for (key, definition) in definitions {
            add(key, definition, &mut variants, &mut added);
        }
    }
    variants
}

fn component_property_name(raw: &str) -> String {
    raw.split('#').next().unwrap_or(raw).to_owned()
}

pub(super) fn legacy_component_name(input: &str) -> String {
    if !input.is_empty()
        && input
            .chars()
            .all(|character| !character.is_alphabetic() || character.is_uppercase())
    {
        let mut characters = input.chars();
        if let Some(first) = characters.next() {
            return first
                .to_uppercase()
                .chain(characters.flat_map(char::to_lowercase))
                .collect();
        }
    }
    let mut output = String::new();
    for part in input
        .split(['-', '_', '/', ' '])
        .filter(|part| !part.is_empty())
    {
        let acronym = part
            .chars()
            .filter(|character| character.is_alphabetic())
            .all(|character| character.is_uppercase());
        let normalized = if acronym {
            part.to_lowercase()
        } else {
            part.to_owned()
        };
        let mut characters = normalized.chars();
        if let Some(first) = characters.next() {
            output.extend(first.to_uppercase());
            output.extend(characters);
        }
    }
    if output.is_empty() {
        "FigmaComponent".to_owned()
    } else {
        output
    }
}

pub fn generate_node(
    snapshot: &Snapshot,
    root_id: &str,
    options: &CodegenOptions,
) -> Result<CodegenOutput, DevupError> {
    let root = snapshot.nodes.get(root_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Figma snapshot에서 변환할 node를 찾지 못했습니다.",
            false,
        )
    })?;
    let render_root = if root.typed_view().node_type() == "SECTION" {
        root.typed_view()
            .child_ids()
            .next()
            .and_then(|id| snapshot.nodes.get(id))
            .unwrap_or(root)
    } else {
        root
    };
    let mut context = Context {
        inline_instances: options.inline_instances,
        text_style_tokens: options.text_style_tokens.clone(),
        variable_tokens: options.variable_tokens.clone(),
        ..Context::default()
    };
    let jsx = render_node(snapshot, render_root, 0, &mut context, &mut HashSet::new())?;
    let imports = context.imports.iter().cloned().collect::<Vec<_>>();
    Ok(CodegenOutput {
        tsx: jsx,
        imports,
        used_tokens: context.used_tokens,
        diagnostics: context.diagnostics,
    })
}

#[derive(Default)]
struct Context {
    imports: BTreeSet<String>,
    used_tokens: BTreeSet<String>,
    diagnostics: Vec<Diagnostic>,
    inline_instances: bool,
    text_style_tokens: std::collections::BTreeMap<String, String>,
    variable_tokens: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PropValue {
    String(String),
}

pub(super) type Prop = (String, PropValue);

fn render_node(
    snapshot: &Snapshot,
    node: &RawNode,
    depth: usize,
    context: &mut Context,
    visiting: &mut HashSet<String>,
) -> Result<String, DevupError> {
    if !visiting.insert(node.id.clone()) {
        return Err(DevupError::new(
            ErrorCode::DevupCodegenFailed,
            "Figma node 트리에 순환 참조가 있습니다.",
            false,
        ));
    }
    add_fallback_diagnostics(node, context);
    let view = node.typed_view();
    if view.node_type() == "INSTANCE" && !context.inline_instances {
        let references = view
            .value("componentPropertyReferences")
            .and_then(serde_json::Value::as_object);
        let main_property = references
            .and_then(|references| references.get("mainComponent"))
            .and_then(serde_json::Value::as_str)
            .map(component_property_name);
        let visible_property = references
            .and_then(|references| references.get("visible"))
            .and_then(serde_json::Value::as_str)
            .map(component_property_name);
        let component = snapshot
            .nodes
            .values()
            .filter(|candidate| candidate.typed_view().node_type() == "COMPONENT")
            .filter_map(|candidate| candidate.typed_view().name())
            .filter(|name| {
                view.name()
                    .is_some_and(|instance| instance.starts_with(name))
            })
            .max_by_key(|name| name.len())
            .map(legacy_component_name)
            .unwrap_or_else(|| legacy_component_name(view.name().unwrap_or("Component")));
        let content = main_property.unwrap_or_else(|| format!("<{component} />"));
        let expression = if let Some(visible) = visible_property {
            format!("{{{visible} && {content}}}")
        } else if references.is_some_and(|references| references.contains_key("mainComponent")) {
            format!("{{{content}}}")
        } else {
            content
        };
        if view.string("layoutPositioning") == Some("ABSOLUTE") {
            let mut props = Vec::new();
            layout::push_layout_props(snapshot, node, "Box", &mut props);
            props.retain(|(name, value)| {
                matches!(
                    name.as_str(),
                    "pos" | "left" | "right" | "top" | "bottom" | "transform" | "transformOrigin"
                ) || (name == "w" && matches!(value, PropValue::String(value) if value == "100%"))
            });
            context.imports.insert("Box".to_owned());
            let indent = "  ".repeat(depth);
            let (opening_props, multiline_props) = render_props(&props, depth);
            let close_open = if multiline_props {
                format!("{opening_props}\n{indent}>")
            } else {
                format!("{opening_props}>")
            };
            visiting.remove(&node.id);
            return Ok(format!(
                "{indent}<Box{close_open}\n{}{}\n{indent}</Box>",
                "  ".repeat(depth + 1),
                expression
            ));
        }
        visiting.remove(&node.id);
        return Ok(format!("{}{expression}", "  ".repeat(depth)));
    }
    let asset = style::asset_kind(snapshot, node);
    let inferred_mode = view
        .value("inferredAutoLayout")
        .and_then(serde_json::Value::as_object)
        .and_then(|layout| layout.get("layoutMode"))
        .and_then(serde_json::Value::as_str);
    let inferred_align = |name: &str| {
        view.string(name).or_else(|| {
            view.value("inferredAutoLayout")
                .and_then(serde_json::Value::as_object)
                .and_then(|layout| layout.get(name))
                .and_then(serde_json::Value::as_str)
        })
    };
    let component = if asset == Some(style::AssetKind::SvgMask) {
        "Box"
    } else if asset.is_some() {
        "Image"
    } else if view.node_type() == "TEXT" {
        "Text"
    } else {
        match inferred_mode {
            Some("GRID") => "Grid",
            Some("HORIZONTAL" | "VERTICAL")
                if inferred_align("primaryAxisAlignItems") == Some("CENTER")
                    && inferred_align("counterAxisAlignItems") == Some("CENTER") =>
            {
                "Center"
            }
            Some("VERTICAL") => "VStack",
            Some("HORIZONTAL") => "Flex",
            _ => "Box",
        }
    };
    context.imports.insert(component.to_owned());

    let mut props = Vec::new();
    layout::push_layout_props(snapshot, node, component, &mut props);
    if asset.is_none()
        && view.value("inferredAutoLayout").is_none()
        && view.string("layoutPositioning") == Some("AUTO")
        && view.child_ids().any(|child| {
            snapshot
                .nodes
                .get(child)
                .is_some_and(|child| child.typed_view().string("layoutPositioning") == Some("AUTO"))
        })
    {
        layout::string_prop(&mut props, "pos", "relative");
    }
    style::push_style_props(
        snapshot,
        node,
        component,
        asset,
        &mut props,
        &mut context.used_tokens,
        &context.variable_tokens,
    );
    text::push_text_props(
        &view,
        &context.text_style_tokens,
        &context.variable_tokens,
        &mut context.used_tokens,
        &mut props,
    );
    if asset.is_some() {
        props.retain(|(name, _)| {
            !matches!(
                name.as_str(),
                "alignItems"
                    | "justifyContent"
                    | "flexDir"
                    | "gap"
                    | "outline"
                    | "outlineOffset"
                    | "overflow"
                    | "p"
                    | "px"
                    | "py"
                    | "pt"
                    | "pr"
                    | "pb"
                    | "pl"
            )
        });
    }
    let indent = "  ".repeat(depth);
    let children = if asset.is_some() {
        Vec::new()
    } else {
        view.child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .map(|child| render_node(snapshot, child, depth + 1, context, visiting))
            .collect::<Result<Vec<_>, _>>()?
    };

    let (opening_props, multiline_props) = render_props(&props, depth);
    let rendered = if component == "Text" {
        let children = text::render_text_children(
            &view,
            &context.text_style_tokens,
            &context.variable_tokens,
            &mut context.used_tokens,
            depth + 1,
        );
        let close_open = if multiline_props {
            format!("{opening_props}\n{indent}>")
        } else {
            format!("{opening_props}>")
        };
        format!("{indent}<Text{close_open}\n{children}\n{indent}</Text>")
    } else if children.is_empty() {
        if multiline_props {
            format!("{indent}<{component}{opening_props}\n{indent}/>")
        } else {
            format!("{indent}<{component}{opening_props} />")
        }
    } else {
        let close_open = if multiline_props {
            format!("{opening_props}\n{indent}>")
        } else {
            format!("{opening_props}>")
        };
        format!(
            "{indent}<{component}{close_open}\n{}\n{indent}</{component}>",
            children.join("\n")
        )
    };
    let rendered = if view.node_type() == "INSTANCE" && context.inline_instances {
        rendered
    } else {
        view.value("componentPropertyReferences")
            .and_then(serde_json::Value::as_object)
            .and_then(|references| references.get("visible"))
            .and_then(serde_json::Value::as_str)
            .map(component_property_name)
            .map(|property| {
                let content = rendered.strip_prefix(&indent).unwrap_or(&rendered);
                format!("{indent}{{{property} && {content}}}")
            })
            .unwrap_or(rendered)
    };
    visiting.remove(&node.id);
    Ok(rendered)
}

fn named_tokens(result: Option<&UpstreamResult>, collection: &str) -> BTreeMap<String, String> {
    fn visit(value: &serde_json::Value, collection: &str, tokens: &mut BTreeMap<String, String>) {
        if let Some(values) = value.get(collection).and_then(serde_json::Value::as_array) {
            for value in values {
                if let (Some(id), Some(name)) = (
                    value.get("id").and_then(serde_json::Value::as_str),
                    value.get("name").and_then(serde_json::Value::as_str),
                ) {
                    tokens.insert(id.to_owned(), to_camel(name));
                }
            }
        }
        match value {
            serde_json::Value::Object(object) => {
                object
                    .values()
                    .for_each(|value| visit(value, collection, tokens));
            }
            serde_json::Value::Array(values) => {
                values
                    .iter()
                    .for_each(|value| visit(value, collection, tokens));
            }
            _ => {}
        }
    }
    let mut tokens = BTreeMap::new();
    if let Some(result) = result {
        visit(&result.raw, collection, &mut tokens);
    }
    tokens
}

fn to_camel(input: &str) -> String {
    let mut output = String::new();
    for (index, part) in input
        .split(|character: char| !character.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .enumerate()
    {
        let mut characters = part.chars();
        if let Some(first) = characters.next() {
            if index == 0 {
                output.extend(first.to_lowercase());
            } else {
                output.extend(first.to_uppercase());
            }
            output.extend(characters);
        }
    }
    output
}

fn render_props(props: &[Prop], depth: usize) -> (String, bool) {
    if props.is_empty() {
        return (String::new(), false);
    }
    let mut sorted = props.to_vec();
    sorted.sort_by(|left, right| left.0.cmp(&right.0));
    let rendered = sorted
        .into_iter()
        .map(|(name, value)| match value {
            PropValue::String(value) => format!("{name}=\"{value}\""),
        })
        .collect::<Vec<_>>();
    let multiline = rendered.len() >= 5;
    if multiline {
        let prefix = "  ".repeat(depth + 1);
        (
            format!("\n{prefix}{}", rendered.join(&format!("\n{prefix}"))),
            true,
        )
    } else {
        (format!(" {}", rendered.join(" ")), false)
    }
}

fn add_fallback_diagnostics(node: &RawNode, context: &mut Context) {
    let view = node.typed_view();
    let candidates = [
        (
            view.bool("isMask") == Some(true),
            "DEVUP_CODEGEN_MASK_FALLBACK",
            "Mask는 기본 Box 렌더링으로 보존됩니다.",
        ),
        (
            view.string("layoutPositioning") == Some("ABSOLUTE"),
            "DEVUP_CODEGEN_ABSOLUTE_FALLBACK",
            "절대 배치는 position props로 제한적으로 변환됩니다.",
        ),
        (
            view.value("effects")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|effects| !effects.is_empty()),
            "DEVUP_CODEGEN_EFFECT_FALLBACK",
            "일부 Figma effect는 계산된 CSS로 변환되지 않을 수 있습니다.",
        ),
    ];
    for (enabled, code, message) in candidates {
        if enabled {
            context.diagnostics.push(Diagnostic {
                code: code.to_owned(),
                message: message.to_owned(),
                node_id: Some(node.id.clone()),
            });
        }
    }
}

pub fn normalize_component_name(input: &str) -> String {
    let mut result = String::new();
    let mut segment = String::new();
    for character in input.chars().chain(std::iter::once(' ')) {
        if character.is_alphanumeric() || character == '_' {
            segment.push(character);
            continue;
        }
        if !segment.is_empty() {
            let all_ascii_upper = segment
                .chars()
                .filter(|character| character.is_ascii_alphabetic())
                .all(|character| character.is_ascii_uppercase());
            let mut characters = segment.chars();
            if let Some(first) = characters.next() {
                result.extend(first.to_uppercase());
                for rest in characters {
                    if all_ascii_upper {
                        result.extend(rest.to_lowercase());
                    } else {
                        result.push(rest);
                    }
                }
            }
            segment.clear();
        }
    }
    if result.is_empty() {
        return "FigmaComponent".to_owned();
    }
    if result
        .chars()
        .next()
        .is_some_and(|character| character.is_numeric())
    {
        result.insert(0, '_');
    }
    result
}
