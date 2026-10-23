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
    /// Actual acquired/requested representation per instance-specific asset id.
    pub asset_policies: BTreeMap<String, super::AssetRepresentationPolicy>,
    pub component_name: Option<String>,
    pub include_diagnostics: bool,
    pub inline_instances: bool,
    pub text_style_tokens: std::collections::BTreeMap<String, String>,
    /// Materialized typography values from the same resources as devup.json.
    /// Names alone cannot prove that a local font property is redundant.
    pub typography_values: BTreeMap<String, serde_json::Value>,
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
    /// Opt-in width-change anchors; `None` keeps the drawn-width output.
    pub width_policy: Option<super::width::WidthPolicy>,
    /// Length tokens of the theme generated from the same variables, by the
    /// id of the variable named for them. A length bound to one of those
    /// variables is written as its `$token` where the token comes to the
    /// drawn value at the width the screen was drawn for.
    pub length_tokens: BTreeMap<String, LengthToken>,
}

/// A `length` token as the theme writes it, in the theme a page shows
/// without `data-theme`: one length, or a responsive array of them.
#[derive(Debug, Clone, PartialEq)]
pub struct LengthToken {
    pub token: String,
    pub value: serde_json::Value,
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
        self.length_tokens = length_tokens(variables);
        let mut snapshot = variables
            .and_then(|result| crate::theme::variable_snapshot_from_result(result).ok())
            .unwrap_or_else(|| {
                serde_json::from_value(serde_json::json!({})).expect("empty variable snapshot")
            });
        if let Some(styles) =
            styles.and_then(|result| crate::theme::variable_snapshot_from_result(result).ok())
        {
            for style in styles.styles {
                if !snapshot
                    .styles
                    .iter()
                    .any(|existing| existing.id == style.id)
                {
                    snapshot.styles.push(style);
                }
            }
        }
        self.typography_values =
            crate::theme::generate_devup_json(&snapshot, crate::theme::ThemeScope::File)
                .ok()
                .and_then(|output| serde_json::from_str::<serde_json::Value>(&output.json).ok())
                .and_then(|value| {
                    value
                        .pointer("/theme/typography")
                        .and_then(serde_json::Value::as_object)
                        .cloned()
                })
                .map(|values| values.into_iter().collect())
                .unwrap_or_default();
        self
    }
}

/// The length tokens the theme of these variables defines, by variable id:
/// the same generator and scope the export's `devupJson` uses, so a token
/// written into the code is the one the theme defines, with its value.
fn length_tokens(variables: Option<&UpstreamResult>) -> BTreeMap<String, LengthToken> {
    let Some(snapshot) =
        variables.and_then(|result| crate::theme::variable_snapshot_from_result(result).ok())
    else {
        return BTreeMap::new();
    };
    let Some(theme) = crate::theme::generate_devup_json(&snapshot, crate::theme::ThemeScope::Node)
        .ok()
        .and_then(|output| serde_json::from_str::<serde_json::Value>(&output.json).ok())
    else {
        return BTreeMap::new();
    };
    // devup-ui's default theme is the first one under `colors`.
    let lengths = &theme["theme"]["length"];
    let Some(lengths) = theme["theme"]["colors"]
        .as_object()
        .and_then(|themes| themes.keys().next())
        .or_else(|| lengths.as_object().and_then(|themes| themes.keys().next()))
        .and_then(|name| lengths.get(name))
        .and_then(serde_json::Value::as_object)
    else {
        return BTreeMap::new();
    };
    snapshot
        .variables
        .iter()
        .chain(&snapshot.used_remote_variables)
        .filter_map(|variable| {
            let token = variable_token(
                &variable.name,
                variable.code_syntax.get("WEB").map(String::as_str),
            );
            let value = lengths.get(&token)?.clone();
            Some((variable.id.clone(), LengthToken { token, value }))
        })
        .collect()
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
    /// The policy the output was generated under, so verification and the
    /// width report judge the anchors it actually wrote.
    pub width_policy: Option<super::width::WidthPolicy>,
}

pub fn generate_component(
    snapshot: &Snapshot,
    root_id: &str,
    options: &CodegenOptions,
) -> Result<CodegenOutput, DevupError> {
    if let Some(root) = snapshot.nodes.get(root_id)
        && root.node_type == "COMPONENT_SET"
    {
        return match variant::generate_variant_component_set(snapshot, root_id, options, true)? {
            Some(mut output) => {
                output.tsx.insert_str(0, &format!("import {{ {} }} from \"@devup-ui/react\";\n\n", output.imports.join(", ")));
                finalize_codegen_output(output, snapshot, options, root_id)
            },
            None => generate_component_set_target(snapshot, root_id, root.typed_view().name().unwrap_or("Component"), options),
        };
    }
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
        && let Some(output) = variant::generate_variant_component_set(snapshot, &set.id, options, false)?
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
        include_str!("../../text/component-render_component_registration_snapshot-dcc3fe0a.md"),
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
