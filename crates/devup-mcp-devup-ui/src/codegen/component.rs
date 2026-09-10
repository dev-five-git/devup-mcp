use std::collections::{BTreeMap, BTreeSet, HashSet};

use devup_mcp_figma::{
    CollectedPayload, DevupError, Diagnostic, DiagnosticSeverity, ErrorCode, RawNode, Snapshot,
    UpstreamResult,
};
use serde::{Deserialize, Serialize};

use super::{animation, layout, style, text, variant};
use crate::provenance::{
    FidelityReport, ProjectionTrace, SourceMap, build_projection_trace, finalize_tsx, mark_node,
    validate_fidelity,
};
use crate::theme::variable_token;
use crate::validation::validate_tsx;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RootLayout {
    #[default]
    Standalone,
    Embedded,
}

#[derive(Debug, Clone, Default)]
pub struct CodegenOptions {
    pub component_name: Option<String>,
    pub include_diagnostics: bool,
    pub inline_instances: bool,
    pub text_style_tokens: std::collections::BTreeMap<String, String>,
    pub variable_tokens: std::collections::BTreeMap<String, String>,
    pub root_layout: RootLayout,
    /// Name every asset after the node it came from, rather than after its
    /// layer. Off by default, which is how the plugin names them: a layer
    /// name is the file name, and two nodes named alike share a file.
    ///
    /// That sharing is a loss wherever the two are not the same picture. A
    /// designer names three logos `Logo`, and only one of them can be
    /// written; a photograph drawn at three widths is one file, so at two of
    /// them the file is the wrong size for the box and the picture is
    /// stretched into it. Named per node, each gets a file of its own.
    pub asset_names_per_node: bool,
}

impl CodegenOptions {
    pub fn with_payload_tokens(self, payload: &CollectedPayload) -> Self {
        self.with_resource_results(payload.variables.as_ref(), payload.styles.as_ref())
    }

    /// The two collected resources on their own, for a caller that kept them
    /// without the rest of the payload — a capture replayed from a fixture
    /// has its variables and styles but no live target, stats or assets to
    /// rebuild a `CollectedPayload` around them.
    pub fn with_resource_results(
        mut self,
        variables: Option<&UpstreamResult>,
        styles: Option<&UpstreamResult>,
    ) -> Self {
        self.text_style_tokens = named_tokens(styles, "styles");
        self.variable_tokens = named_tokens(variables, "variables");
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenOutput {
    pub tsx: String,
    pub imports: Vec<String>,
    pub used_tokens: BTreeSet<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub source_map: SourceMap,
    pub projection_trace: ProjectionTrace,
    pub fidelity_report: FidelityReport,
}

pub fn generate_component(
    snapshot: &Snapshot,
    root_id: &str,
    options: &CodegenOptions,
) -> Result<CodegenOutput, DevupError> {
    let generated = generate_node_marked(snapshot, root_id, options)?;
    let root = snapshot.nodes.get(root_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Node to convert was not found in the Figma snapshot.",
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
        "import {{ {} }} from \"@devup-ui/react\";\n",
        generated.imports.join(", ")
    );
    // Naming a component without importing it produces code that reads well and
    // does not compile. When instances are left as references, whatever they
    // refer to has to be resolvable, and the project convention is one named
    // export per file under `@/components`.
    for name in referenced_components(&generated.tsx) {
        tsx.push_str(&format!(
            "import {{ {name} }} from \"@/components/{name}\";\n"
        ));
    }
    tsx.push('\n');
    tsx.push_str(&format!(
        "export function {component_name}() {{\n  return (\n{body}\n  );\n}}\n"
    ));
    finalize_codegen_output(
        CodegenOutput { tsx, ..generated },
        snapshot,
        options,
        root_id,
    )
}

pub fn generate_legacy_component(
    snapshot: &Snapshot,
    root_id: &str,
    options: &CodegenOptions,
) -> Result<CodegenOutput, DevupError> {
    let generated = generate_node_marked(snapshot, root_id, options)?;
    let root = snapshot.nodes.get(root_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Node to convert was not found in the Figma snapshot.",
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
    finalize_codegen_output(
        CodegenOutput {
            tsx: format!("export function {component_name}() {{\n  return {body}\n}}"),
            ..generated
        },
        snapshot,
        options,
        root_id,
    )
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
            "Component set was not found in the Figma snapshot.",
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
        return finalize_codegen_output(output, snapshot, options, &set.id);
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
            format!("Output '{target_name}' was not found in the component set."),
            false,
        )
    })?;

    let (generated, provenance_root_id) = if main {
        match generate_component_asset_child(snapshot, target_id, options) {
            Some(output) => output?,
            None => (
                generate_node_marked(snapshot, target_id, options)?,
                target_id.to_owned(),
            ),
        }
    } else {
        (
            generate_node_marked(snapshot, target_id, options)?,
            target_id.to_owned(),
        )
    };
    let code = if !main && generated.tsx.contains("<Box />") {
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
    finalize_codegen_output(
        CodegenOutput {
            tsx: render_component_source(&legacy_component_name(target_name), &code, &variants),
            ..generated
        },
        snapshot,
        options,
        &provenance_root_id,
    )
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
            "Inline instance root was not found.",
            false,
        )
    })?;
    let instance = snapshot.nodes.get(instance_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Component instance to inline was not found.",
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
                format!("Component set '{name}' was not found."),
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
                format!("Instance variant '{name}' was not found."),
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
    let mut output = generate_node_marked(&projected, &selected.id, options)?;
    if let Some(close) = output.tsx.rfind("\n/>") {
        let suffix = output.tsx[close + 3..].to_owned();
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
        output.tsx = format!("{}\n/>{suffix}", lines.join("\n"));
    }
    let usage = selected_variants
        .iter()
        .map(|(key, value)| format!(" {key}=\"{value}\""))
        .collect::<String>();
    output.tsx = format!("{{/* <{name}{usage} /> */}}\n{}", output.tsx);
    finalize_codegen_output(output, &projected, options, &selected.id)
}

fn generate_component_asset_child(
    snapshot: &Snapshot,
    component_id: &str,
    options: &CodegenOptions,
) -> Option<Result<(CodegenOutput, String), DevupError>> {
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
    Some(
        generate_node_marked(&projected, child_id, options)
            .map(|output| (output, child_id.to_owned())),
    )
}

pub fn render_component_registration_snapshot(
    snapshot: &Snapshot,
    root_id: &str,
    target_name: &str,
) -> Result<String, DevupError> {
    let root = snapshot.nodes.get(root_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Component registration root was not found.",
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
                    format!("Registration target '{target_name}' was not found."),
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
            let candidate = first
                .to_uppercase()
                .chain(characters.flat_map(char::to_lowercase))
                .collect::<String>();
            return sanitize_legacy_identifier(&candidate);
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
    sanitize_legacy_identifier(&output)
}

fn sanitize_legacy_identifier(candidate: &str) -> String {
    let mut output = candidate
        .chars()
        .filter(|character| character.is_alphanumeric() || *character == '_')
        .collect::<String>();
    if output.is_empty() {
        return "FigmaComponent".to_owned();
    }
    if output
        .chars()
        .next()
        .is_some_and(|character| character.is_numeric())
    {
        output.insert(0, '_');
    }
    output
}

pub fn generate_node(
    snapshot: &Snapshot,
    root_id: &str,
    options: &CodegenOptions,
) -> Result<CodegenOutput, DevupError> {
    finalize_codegen_output(
        generate_node_marked(snapshot, root_id, options)?,
        snapshot,
        options,
        root_id,
    )
}

fn generate_node_marked(
    snapshot: &Snapshot,
    root_id: &str,
    options: &CodegenOptions,
) -> Result<CodegenOutput, DevupError> {
    let root = snapshot.nodes.get(root_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Node to convert was not found in the Figma snapshot.",
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
        asset_names_per_node: options.asset_names_per_node,
        inline_instances: options.inline_instances,
        text_style_tokens: options.text_style_tokens.clone(),
        variable_tokens: options.variable_tokens.clone(),
        root_layout: options.root_layout,
        ..Context::default()
    };
    let jsx = render_node(snapshot, render_root, 0, &mut context, &mut HashSet::new())?;
    let imports = context.imports.iter().cloned().collect::<Vec<_>>();
    Ok(CodegenOutput {
        tsx: jsx,
        imports,
        used_tokens: context.used_tokens,
        diagnostics: context.diagnostics,
        source_map: SourceMap::empty(),
        projection_trace: ProjectionTrace::default(),
        fidelity_report: FidelityReport::default(),
    })
}

fn finalize_codegen_output(
    mut output: CodegenOutput,
    snapshot: &Snapshot,
    options: &CodegenOptions,
    root_id: &str,
) -> Result<CodegenOutput, DevupError> {
    let (tsx, source_map) = finalize_tsx(
        &output.tsx,
        snapshot,
        &options.variable_tokens,
        &options.text_style_tokens,
    );
    output.tsx = tsx;
    output.source_map = source_map;
    crate::provenance::account_for_sizing(snapshot, &mut output);
    validate_tsx(&output.tsx)?;
    if let Some(contract) = super::evidence::placement_contract(snapshot, &output, options, root_id)
    {
        output.diagnostics.push(contract);
    }
    // Background and variant projections share paint suppression in style;
    // attach evidence for those paths as well as ordinary asset leaves.
    for entry in output
        .source_map
        .entries
        .iter()
        .filter(|e| e.property.is_none() && e.resolution == "node")
    {
        let Some(node) = entry.node_id.as_ref().and_then(|id| snapshot.nodes.get(id)) else {
            continue;
        };
        let Some(reason) = style::non_rendering_asset_reason(snapshot, node) else {
            continue;
        };
        if output.diagnostics.iter().any(|d| {
            d.code == "DEVUP_CODEGEN_NON_RENDERING_ASSET" && d.node_id.as_deref() == Some(&node.id)
        }) {
            continue;
        }
        let Some(source) = entry
            .generated_range
            .as_ref()
            .and_then(|r| output.tsx.get(r.start..r.end))
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        output.diagnostics.push(Diagnostic {
            code: "DEVUP_CODEGEN_NON_RENDERING_ASSET".into(),
            node_id: Some(node.id.clone()), property: Some("assetReference".into()),
            message: "Non-rendering asset paint omitted while retaining layout without an asset reference.".into(),
            fidelity_impact: Some(devup_mcp_figma::FidelityImpact::Approximated),
            details: Some(serde_json::json!({"originalValue":{"visible":node.typed_view().value("visible"),"opacity":node.typed_view().value("opacity"),"absoluteRenderBounds":node.typed_view().value("absoluteRenderBounds")},
                "appliedValue":{"state":"layout-only","generatedNodeId":node.id,"generatedSource":source.chars().take(800).collect::<String>(),"sourceTruncated":source.chars().count()>800},
                "reason":reason,"classification":"non-rendering-asset",
                "nextAction":"Review the invisible layout element; correct visibility or clipping in Figma if this paint should be visible."})),
            ..Diagnostic::default()
        });
    }
    output
        .diagnostics
        .extend(unresolved_token_bindings(snapshot, root_id, options));
    output.projection_trace =
        build_projection_trace(snapshot, root_id, &output.tsx, &output.source_map);
    output.fidelity_report = validate_fidelity(snapshot, root_id, &output)?;
    output
        .diagnostics
        .retain(|d| d.code != "DEVUP_CODEGEN_LAYOUT_UNCOVERED");
    // A coverage shortfall has no proven intentional-exclusion classification.
    // Report the actual field instead of allowing an exact quality grade.
    for pair in &output.fidelity_report.uncovered_layout {
        if let Some((node_id, property)) = pair.rsplit_once('#') {
            output.diagnostics.push(Diagnostic {
                code: "DEVUP_CODEGEN_LAYOUT_UNCOVERED".into(),
                message: "No verified property mapping accounts for this layout field; generated evidence describes what was emitted.".into(),
                node_id: Some(node_id.into()),
                property: Some(property.into()),
                fidelity_impact: Some(devup_mcp_figma::FidelityImpact::Lossy),
                details: Some(super::evidence::uncovered_layout_details(snapshot, &output, options, node_id, property)),
                ..Diagnostic::default()
            });
        }
    }
    for diagnostic in &mut output.diagnostics {
        let property = match diagnostic.code.as_str() {
            "DEVUP_CODEGEN_ABSOLUTE_FALLBACK" => Some("layoutPositioning"),
            "DEVUP_CODEGEN_MASK_FALLBACK" => Some("isMask"),
            "DEVUP_CODEGEN_EFFECT_FALLBACK" => Some("effects"),
            "DEVUP_CODEGEN_ANIMATION_UNREACHABLE" => Some("reactions"),
            "DEVUP_CODEGEN_VARIANT_CHILD_FALLBACK" => Some("childrenIds"),
            "DEVUP_CODEGEN_TOKEN_NAME_UNRESOLVED" => diagnostic.property.as_deref(),
            _ => None,
        };
        if let Some(property) = property.map(str::to_owned) {
            diagnostic.property = Some(property.clone());
            let node_id = diagnostic.node_id.as_deref().unwrap_or(root_id);
            let generated = output
                .source_map
                .entries
                .iter()
                .filter(|entry| {
                    entry.node_id.as_deref() == Some(node_id) && entry.property.is_none()
                })
                .filter_map(|entry| entry.generated_range.as_ref())
                .filter_map(|range| output.tsx.get(range.start..range.end))
                .collect::<Vec<_>>();
            diagnostic.details = Some(serde_json::json!({
                "originalValue": super::evidence::fallback_original(snapshot, node_id, &property),
                "originalResourceId": diagnostic.resource_id,
                "appliedValue": {"generatedSource": generated, "fallback": diagnostic.fallback,
                    "derivedPadding": if property == "layoutPositioning" {
                        snapshot.nodes.get(node_id).and_then(|node|layout::derived_padding(snapshot,node))
                            .map(|[top,right,bottom,left]|serde_json::json!({"top":top,"right":right,"bottom":bottom,"left":left,
                                "basis":"Generator-derived inset from visible child geometry; not the source padding fields."}))
                    } else { None }},
                "evidenceLimit": "Generated source and collected geometry are comparison evidence, not measured browser layout or proof of responsive equivalence.",
                "stage": "projection"
            }));
        }
    }
    output.fidelity_report = validate_fidelity(snapshot, root_id, &output)?;
    Ok(output)
}

/// Reports every binding the generated code had to write out as a value.
///
/// A fill bound to a variable, or a text carrying a style, is the design
/// saying "this is a token". The generator writes the token when the resource
/// catalog carried that variable or style, and the resolved value when it did
/// not - and it has to write something, because the module still has to
/// compile and render. What it must not do is stay quiet about it: a
/// hardcoded `#7d7f83` where the design says `$caption` renders identically
/// today and stops following the theme tomorrow, and a caller reading a
/// response graded `exact` has no way to know one is in there.
///
/// Run as a pass over the collected subtree rather than inside rendering, so
/// it sees the node a binding belongs to and needs no argument threaded
/// through the render functions to reach it.
fn unresolved_token_bindings(
    snapshot: &Snapshot,
    root_id: &str,
    options: &CodegenOptions,
) -> Vec<Diagnostic> {
    fn paint_variable_ids(fills: Option<&serde_json::Value>) -> Vec<String> {
        fills
            .and_then(serde_json::Value::as_array)
            .map(|paints| {
                paints
                    .iter()
                    .filter_map(|paint| {
                        Some(
                            paint
                                .get("boundVariables")?
                                .get("color")?
                                .get("id")?
                                .as_str()?
                                .to_owned(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    let mut reported = BTreeSet::new();
    let mut diagnostics = Vec::new();
    let mut pending = vec![root_id.to_owned()];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(node) = snapshot.nodes.get(&id) else {
            continue;
        };
        let view = node.typed_view();
        pending.extend(view.child_ids().map(str::to_owned));

        let mut lost = Vec::new();
        for variable_id in paint_variable_ids(view.value("fills")) {
            if !options.variable_tokens.contains_key(&variable_id) {
                lost.push(("variable", "fills", variable_id));
            }
        }
        if let Some(segments) = view
            .value("styledTextSegments")
            .and_then(serde_json::Value::as_array)
        {
            for segment in segments {
                for variable_id in paint_variable_ids(segment.get("fills")) {
                    if !options.variable_tokens.contains_key(&variable_id) {
                        lost.push(("variable", "fills", variable_id));
                    }
                }
                if let Some(style_id) = segment
                    .get("textStyleId")
                    .and_then(serde_json::Value::as_str)
                    && !style_id.is_empty()
                    && !options.text_style_tokens.contains_key(style_id)
                {
                    lost.push(("textStyle", "textStyleId", style_id.to_owned()));
                }
            }
        }
        if let Some(style_id) = view.string("textStyleId")
            && !style_id.is_empty()
            && !options.text_style_tokens.contains_key(style_id)
        {
            lost.push(("textStyle", "textStyleId", style_id.to_owned()));
        }

        for (kind, property, resource_id) in lost {
            if !reported.insert((id.clone(), property, resource_id.clone())) {
                continue;
            }
            diagnostics.push(Diagnostic {
                code: "DEVUP_CODEGEN_TOKEN_NAME_UNRESOLVED".to_owned(),
                message: format!(
                    "{id} binds {property} to a {kind} the resource catalog did not name, \
                     so the resolved value was written instead of its token. Collect the \
                     resource, or treat the value in the generated code as a token that \
                     still needs a name."
                ),
                severity: Some(DiagnosticSeverity::Warning),
                node_id: Some(id.clone()),
                property: Some(property.to_owned()),
                resource_id: Some(resource_id.clone()),
                resource_kind: Some(kind.to_owned()),
                details: Some(serde_json::json!({
                    "nodeId": id,
                    "property": property,
                    "resourceId": resource_id
                })),
                ..Diagnostic::default()
            });
        }
    }
    diagnostics.sort_by(|left, right| left.message.cmp(&right.message));
    diagnostics
}

#[derive(Default)]
struct Context {
    asset_names_per_node: bool,
    imports: BTreeSet<String>,
    used_tokens: BTreeSet<String>,
    diagnostics: Vec<Diagnostic>,
    inline_instances: bool,
    instance_boolean_properties: Vec<BTreeMap<String, bool>>,
    text_style_tokens: std::collections::BTreeMap<String, String>,
    variable_tokens: std::collections::BTreeMap<String, String>,
    root_layout: RootLayout,
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
            "Figma node tree contains a circular reference.",
            false,
        ));
    }
    let view = node.typed_view();
    // Hidden snapshot subtrees have no deliverable assets. Do not emit their
    // image/background/mask references, even under display:none.
    if view.bool("visible") == Some(false) {
        visiting.remove(&node.id);
        return Ok(mark_node(
            &node.id,
            if depth == 0 {
                "<></>".into()
            } else {
                String::new()
            },
        ));
    }
    add_fallback_diagnostics(snapshot, node, context);
    let concrete_visibility = if context.inline_instances {
        view.value("componentPropertyReferences")
            .and_then(serde_json::Value::as_object)
            .and_then(|references| references.get("visible"))
            .and_then(serde_json::Value::as_str)
            .and_then(|property| {
                context
                    .instance_boolean_properties
                    .iter()
                    .rev()
                    .find_map(|properties| properties.get(property).copied())
            })
    } else {
        None
    };
    if concrete_visibility == Some(false) {
        visiting.remove(&node.id);
        return Ok(mark_node(&node.id, String::new()));
    }
    if view.node_type() == "INSTANCE"
        && !context.inline_instances
        && style::non_rendering_asset_reason(snapshot, node).is_none()
    {
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
            layout::push_layout_props(
                snapshot,
                node,
                "Box",
                &mut props,
                context.root_layout,
                depth == 0,
            );
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
            return Ok(mark_node(
                &node.id,
                format!(
                    "{indent}<Box{close_open}\n{}{}\n{indent}</Box>",
                    "  ".repeat(depth + 1),
                    expression
                ),
            ));
        }
        visiting.remove(&node.id);
        return Ok(mark_node(
            &node.id,
            format!("{}{expression}", "  ".repeat(depth)),
        ));
    }
    let asset = style::asset_kind(snapshot, node);
    if asset.is_some()
        && let Some(reason) = devup_mcp_figma::asset_exclusion_reason(node)
    {
        // A transparent/clipped asset still occupies layout space. Keep the
        // asset sizing policy, but emit no external paint or descendant asset.
        let mut props = Vec::new();
        layout::push_layout_props(
            snapshot,
            node,
            "Image",
            &mut props,
            context.root_layout,
            depth == 0,
        );
        layout::string_prop(&mut props, "visibility", "hidden");
        let (opening, multiline) = render_props(&props, depth);
        let indent = "  ".repeat(depth);
        let rendered = if multiline {
            format!("{indent}<Box{opening}\n{indent}/>")
        } else {
            format!("{indent}<Box{opening} />")
        };
        context.imports.insert("Box".into());
        context.diagnostics.push(Diagnostic {
            code: "DEVUP_CODEGEN_NON_RENDERING_ASSET".into(),
            node_id: Some(node.id.clone()),
            property: Some("assetReference".into()),
            message: "Non-rendering asset replaced by an invisible layout box without an asset reference.".into(),
            fidelity_impact: Some(devup_mcp_figma::FidelityImpact::Approximated),
            details: Some(serde_json::json!({
                "originalValue": {"visible":view.value("visible"),"opacity":view.value("opacity"),
                    "absoluteBoundingBox":view.value("absoluteBoundingBox"),"absoluteRenderBounds":view.value("absoluteRenderBounds")},
                "appliedValue": {"generatedSource":rendered,"state":"layout-only"},
                "reason":reason,"classification":"non-rendering-asset",
                "nextAction":"Review the retained layout box; no asset bytes are needed. If the asset should be visible, correct its visibility or clipping in Figma and export again."
            })),
            ..Diagnostic::default()
        });
        visiting.remove(&node.id);
        return Ok(mark_node(&node.id, rendered));
    }
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
    } else if layout::centres_its_only_child(snapshot, node) {
        "Center"
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
    layout::push_layout_props(
        snapshot,
        node,
        component,
        &mut props,
        context.root_layout,
        depth == 0,
    );
    // A frame with no auto-layout places its children itself, and this keeps
    // them resolvable. Once the gap around them is measurable it is emitted as
    // padding instead, which puts them where they belong on its own — so the
    // anchor is only still needed where nothing could be measured, as when the
    // child fills the frame exactly or carries no position of its own.
    // Positioned children, including those of page and embedded roots, are
    // anchored by push_layout_props above. This legacy AUTO fallback only
    // applies to other frames.
    let page_root = snapshot
        .nodes
        .values()
        .find(|candidate| {
            candidate
                .typed_view()
                .child_ids()
                .any(|child| child == node.id)
        })
        .map(|parent| parent.typed_view().node_type())
        .or_else(|| view.string("parentType"))
        .is_some_and(|kind| matches!(kind, "SECTION" | "PAGE" | "COMPONENT_SET"));
    if !(depth == 0 && context.root_layout == RootLayout::Embedded)
        && !page_root
        && asset.is_none()
        && view.value("inferredAutoLayout").is_none()
        && view.string("layoutPositioning") == Some("AUTO")
        && layout::derived_padding(snapshot, node).is_none()
        && !layout::centres_its_only_child(snapshot, node)
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
        style::StyleOptions {
            variable_tokens: &context.variable_tokens,
            asset_names_per_node: context.asset_names_per_node,
        },
    );
    text::push_text_props(
        &view,
        &context.text_style_tokens,
        &context.variable_tokens,
        &mut context.used_tokens,
        &mut props,
    );
    // Last, as the plugin merges `getReactionProps` last: what a timed Smart
    // Animate changes becomes keyframes on the child it changes, or on the
    // frame itself.
    let before = props.len();
    animation::push_animation_props(snapshot, node, &context.variable_tokens, &mut props);
    if props.len() > before {
        context.imports.insert("keyframes".to_owned());
    }
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
    let instance_properties =
        (context.inline_instances && view.node_type() == "INSTANCE").then(|| {
            view.value("componentProperties")
                .and_then(serde_json::Value::as_object)
                .map(|properties| {
                    properties
                        .iter()
                        .filter_map(|(key, property)| {
                            (property.get("type").and_then(serde_json::Value::as_str)
                                == Some("BOOLEAN"))
                            .then(|| {
                                property
                                    .get("value")
                                    .and_then(serde_json::Value::as_bool)
                                    .map(|value| (key.clone(), value))
                            })
                            .flatten()
                        })
                        .collect::<BTreeMap<_, _>>()
                })
                .unwrap_or_default()
        });
    if let Some(properties) = instance_properties {
        context.instance_boolean_properties.push(properties);
    }
    let children_result = if asset.is_some() {
        Ok(Vec::new())
    } else {
        view.child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .filter(|child| child.typed_view().bool("visible") != Some(false))
            .map(|child| render_node(snapshot, child, depth + 1, context, visiting))
            .collect::<Result<Vec<_>, _>>()
    };
    if context.inline_instances && view.node_type() == "INSTANCE" {
        context.instance_boolean_properties.pop();
    }
    let children = children_result?;

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
    let rendered = if concrete_visibility == Some(true)
        || (view.node_type() == "INSTANCE" && context.inline_instances)
    {
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
    Ok(mark_node(&node.id, rendered))
}

/// A style name without a leading group that is only a number.
///
/// `0/` and `3/` in front of a style name are how a Figma library is made to
/// sort in the picker; they are not part of what the style is called, and the
/// reference does not carry them into the token. A group that names something
/// (`typography/`) is part of the name and stays.
fn named_tokens(result: Option<&UpstreamResult>, collection: &str) -> BTreeMap<String, String> {
    fn visit(value: &serde_json::Value, collection: &str, tokens: &mut BTreeMap<String, String>) {
        if let Some(values) = value.get(collection).and_then(serde_json::Value::as_array) {
            for value in values {
                if let (Some(id), Some(name)) = (
                    value.get("id").and_then(serde_json::Value::as_str),
                    value.get("name").and_then(serde_json::Value::as_str),
                ) {
                    let token = if collection == "variables" {
                        variable_token(
                            name,
                            value
                                .get("codeSyntax")
                                .and_then(serde_json::Value::as_object)
                                .and_then(|syntax| syntax.get("WEB"))
                                .and_then(serde_json::Value::as_str),
                        )
                    } else {
                        // The rule `devup.json` names the style by, so the
                        // `typography="…"` written here is a key that exists
                        // there: a leading breakpoint or number says where the
                        // style applies and is dropped, any other group is
                        // kept. See `theme::style_token`.
                        crate::theme::style_token(name).1
                    };
                    tokens.insert(id.to_owned(), token);
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

fn render_props(props: &[Prop], depth: usize) -> (String, bool) {
    if props.is_empty() {
        return (String::new(), false);
    }
    let mut sorted = props.to_vec();
    sorted.sort_by(|left, right| left.0.cmp(&right.0));
    let rendered = sorted
        .into_iter()
        .map(|(name, value)| match value {
            PropValue::String(value) => render_static_attribute(&name, &value),
        })
        .collect::<Vec<_>>();
    // Five props, or one that spans lines — a `keyframes({...})` — and the
    // props go one to a line, which is the plugin's `propsToString` rule.
    let multiline =
        rendered.len() >= 5 || rendered.iter().any(|attribute| attribute.contains('\n'));
    if multiline {
        let prefix = "  ".repeat(depth + 1);
        let padded = rendered
            .iter()
            .map(|attribute| attribute.replace('\n', &format!("\n{prefix}")))
            .collect::<Vec<_>>();
        (
            format!("\n{prefix}{}", padded.join(&format!("\n{prefix}"))),
            true,
        )
    } else {
        (format!(" {}", rendered.join(" ")), false)
    }
}

/// A prop as JSX. A value is a quoted string, except `animationName` holding
/// a `keyframes({...})` call, which is the expression itself — the plugin's
/// `propsToString` makes the same exception, and it is how devup-ui's
/// `keyframes` is meant to be written.
pub(super) fn render_static_attribute(name: &str, value: &str) -> String {
    if name == "animationName" && value.starts_with("keyframes(") {
        return format!("{name}={{{value}}}");
    }
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(character),
        }
    }
    format!("{name}=\"{escaped}\"")
}

fn add_fallback_diagnostics(snapshot: &Snapshot, node: &RawNode, context: &mut Context) {
    use devup_mcp_figma::FidelityImpact;

    let view = node.typed_view();
    let candidates = [
        (
            view.bool("isMask") == Some(true),
            "DEVUP_CODEGEN_MASK_FALLBACK",
            "Mask is preserved as a plain Box rendering.",
            FidelityImpact::Lossy,
        ),
        (
            view.string("layoutPositioning") == Some("ABSOLUTE")
                && !layout::absolute_layout_is_exact(snapshot, node),
            "DEVUP_CODEGEN_ABSOLUTE_FALLBACK",
            "Absolute positioning is converted to position props with limited fidelity.",
            FidelityImpact::Approximated,
        ),
        (
            view.value("effects")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|effects| !effects.is_empty())
                && !style::effects_are_exact(&view),
            "DEVUP_CODEGEN_EFFECT_FALLBACK",
            "Some Figma effects may not be converted into computed CSS.",
            FidelityImpact::Lossy,
        ),
        (
            animation::has_unreachable_destination(snapshot, node),
            "DEVUP_CODEGEN_ANIMATION_UNREACHABLE",
            "A timed Smart Animate points at a frame that was not collected, so no keyframes are written for it.",
            FidelityImpact::Lossy,
        ),
    ];
    for (enabled, code, message, fidelity_impact) in candidates {
        if enabled {
            context.diagnostics.push(Diagnostic {
                code: code.to_owned(),
                message: message.to_owned(),
                node_id: Some(node.id.clone()),
                fidelity_impact: Some(fidelity_impact),
                ..Diagnostic::default()
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

/// The custom components a rendered body refers to, in the order a reader meets
/// them, deduplicated. A devup-ui primitive is imported from the library and is
/// not one of these; anything else opening in PascalCase is.
fn referenced_components(body: &str) -> Vec<String> {
    const PRIMITIVES: [&str; 8] = [
        "Box", "Center", "Flex", "Grid", "Image", "Text", "VStack", "Input",
    ];
    let mut seen = BTreeSet::new();
    let mut found = Vec::new();
    for (index, _) in body.match_indices('<') {
        let rest = &body[index + 1..];
        let name = rest
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect::<String>();
        if name.is_empty()
            || !name.starts_with(|character: char| character.is_ascii_uppercase())
            || PRIMITIVES.contains(&name.as_str())
            || !seen.insert(name.clone())
        {
            continue;
        }
        found.push(name);
    }
    found
}
