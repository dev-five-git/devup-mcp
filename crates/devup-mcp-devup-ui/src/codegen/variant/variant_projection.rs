use std::collections::{BTreeMap, BTreeSet};

use devup_mcp_figma::{
    DevupError, Diagnostic, DiagnosticSeverity, ErrorCode, FidelityImpact, RawNode, Snapshot,
};
use serde_json::Value;
use super::{
    component::{CodegenOptions, CodegenOutput, PropValue, legacy_component_name},
    layout, style, text,
};
use crate::provenance::mark_node;
#[derive(Clone, Debug)]
pub(super) struct Tree {
    pub node_id: String,
    /// What the node is called in Figma. Only used to pair a node with its
    /// counterpart at another width when shape alone cannot tell them apart.
    pub node_name: String,
    pub source_node_ids: BTreeSet<String>,
    pub component: String,
    pub props: BTreeMap<String, String>,
    pub children: Vec<Tree>,
    pub content: Option<String>,
    /// True when `component` names a component to reference rather than a
    /// devup-ui primitive, so the renderer must not descend into it.
    pub is_component: bool,
    /// The boolean property that decides whether this node is drawn, if one
    /// does. Figma keeps it on the child rather than on the set.
    pub visible_when: Option<String>,
    /// The variant options this node is drawn at, when it is not drawn at all
    /// of them. Rendered as the condition guarding it.
    pub drawn_when: Vec<String>,
    /// A JSX comment to write immediately above this node, naming the
    /// component it was spelled out from.
    pub leading_comment: Option<String>,
    /// In a responsive merge, the node each slot's width drew this element
    /// from and that node's text; empty everywhere else.
    pub slots: Vec<Option<(String, Option<String>)>>,
    pub scalar_sources: Vec<(String, String)>,
}
#[derive(Clone, Debug)]
struct Record {
    values: BTreeMap<String, String>,
    tree: Tree,
    node_id: String,
}
#[derive(Clone, Debug)]
struct Definition {
    /// The name as it is written in code.
    name: String,
    default: String,
    options: Vec<String>,
    /// A switch rather than a choice: it has no options, and a child names it
    /// to say when it is drawn.
    boolean: bool,
    /// The name Figma knows it by, which a child's `visible` reference uses and
    /// which may carry a `#id` suffix that `name` has dropped.
    source: String,
}
#[derive(Clone, Debug)]
enum Expression {
    Undefined,
    Literal(String),
    /// A devup-ui responsive array, written out slot by slot, narrowest first.
    /// `None` is the literal `null`, which is not "no value" but "whatever the
    /// slot before it said" — so where a value sits decides the width it starts
    /// applying at, and the slots have to be placed, not merely listed.
    Responsive(Vec<Option<String>>),
    Variant(String, Vec<(String, Expression)>),
    Conditional(String, String, String),
}
pub(super) fn generate_variant_component_set(
    snapshot: &Snapshot,
    set_id: &str,
    options: &CodegenOptions,
    merge_all: bool,
) -> Result<Option<CodegenOutput>, DevupError> {
    let set = snapshot.nodes.get(set_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Variant component set was not found.",
            false,
        )
    })?;
    let mut definitions = definitions(set);
    let state_axis = merge_all.then(|| definitions.iter().find(|definition| {
        definition.name.eq_ignore_ascii_case("state") && definition.options.iter().all(|value| {
            matches!(value.to_ascii_lowercase().as_str(), "default" | "hover" | "active" | "disabled" | "focus")
        })
    }).map(|definition| definition.source.clone())).flatten();
    if let Some(state_axis) = &state_axis {
        for definition in &mut definitions {
            if &definition.source == state_axis {
                definition.name = "effect".into();
                definition.source = "effect".into();
                definition.default.make_ascii_lowercase();
                definition.options.iter_mut().for_each(|value| value.make_ascii_lowercase());
            }
        }
    }
    if !merge_all && !definitions.iter().any(|definition| matches!(definition.name.to_ascii_lowercase().as_str(), "effect" | "viewport")) {
        return Ok(None);
    }
    let mut records = set
        .typed_view()
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .filter_map(|node| {
            let mut values = node
                .typed_view()
                .value("variantProperties")?
                .as_object()?
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
                .collect::<BTreeMap<_, _>>();
            if let Some(state_axis) = &state_axis
                && let Some(value) = values.remove(state_axis)
            {
                values.insert("effect".into(), value.to_ascii_lowercase());
            }
            Some((node, values))
        })
        .map(|(node, values)| {
            let mut tree = project_tree(snapshot, node, options, true)?;
            tree.props.retain(|name, _| !POSITION_PROPS.contains(&name.as_str()));
            if layout::holds_positioned_children(snapshot, node) {
                tree.props.insert("pos".into(), "relative".into());
            }
            Ok(Record {
                values,
                tree,
                node_id: node.id.clone(),
            })
        })
        .collect::<Result<Vec<_>, DevupError>>()?;
    if records.is_empty() {
        return Ok(None);
    }
    if records.iter().any(|record| record.tree.component == "Image")
        && records.iter().all(|record| is_asset_leaf(&record.tree))
    {
        for record in &mut records {
            if let Some(mask) = record.tree.props.remove("maskImage") {
                let source = mask.trim_start_matches("url(").trim_end_matches(')').to_owned();
                record.tree.component = "Image".into();
                record.tree.props.retain(|name, _| !matches!(name.as_str(), "bg" | "maskPos" | "maskRepeat" | "maskSize"));
                record.tree.props.insert("src".into(), source);
            }
        }
    }
    // A boolean property is not part of a variant's identity — it never appears
    // in `variantProperties` — so filtering records by it matches nothing.
    let default_filters = definitions
        .iter()
        .filter(|definition| !definition.boolean)
        .map(|definition| (definition.source.clone(), definition.default.clone()))
        .collect::<BTreeMap<_, _>>();
    let default = find_record(&records, &default_filters)
        .or_else(|| records.first())
        .ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupCodegenFailed,
                "Component set has no variant components.",
                false,
            )
        })?;
    let mut dimensions = definitions
        .iter()
        .filter(|definition| {
            !matches!(
                definition.name.to_ascii_lowercase().as_str(),
                "effect" | "viewport"
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let effect = definitions
        .iter()
        .find(|definition| definition.name.eq_ignore_ascii_case("effect") && !records.iter().any(|record| record.tree.props.contains_key("src")) && definition.options.iter().all(|value| value.eq_ignore_ascii_case(&definition.default) || effect_selector(value).is_some()));
    if effect.is_none()
        && let Some(definition) = definitions.iter().find(|definition| definition.name.eq_ignore_ascii_case("effect"))
    {
        dimensions.push(definition.clone());
    }
    let viewport = definitions
        .iter()
        .find(|definition| definition.name.eq_ignore_ascii_case("viewport"));

    let mut root = default.tree.clone();
    let mut unrepresented_variant_nodes = BTreeSet::new();
    merge_source_node_ids(
        &mut root,
        &records,
        &[],
        &mut unrepresented_variant_nodes,
        effect.map(|definition| definition.source.as_str()),
        &default.values,
    );
    root.props = merged_props(
        &records,
        &definitions,
        &dimensions,
        viewport,
        effect.map_or("default", |definition| definition.default.as_str()),
        None,
        &default.tree.props,
    );
    if root.component == "Image" {
        root.scalar_sources = scalar_sources(&records, &definitions, &dimensions, viewport, effect.map_or("default", |definition| definition.default.as_str()), &[]);
    }
    merge_child_props(
        &mut root,
        &records,
        &definitions,
        &dimensions,
        viewport,
        effect.map_or("default", |definition| definition.default.as_str()),
        &[],
    );
    bind_referenced_properties(&mut root, &mut dimensions);

    let mut selectors = BTreeMap::new();
    let mut transition_props = BTreeSet::new();
    if let Some(effect) = effect {
        for effect_value in &effect.options {
            if effect_value == &effect.default {
                continue;
            }
            let changed = changed_effect_props(
                &records,
                &definitions,
                &dimensions,
                viewport,
                &effect.default,
                effect_value,
            );
            if !changed.is_empty() {
                transition_props.extend(changed.keys().cloned());
                if let Some(selector) = effect_selector(effect_value) {
                    selectors.insert(selector.to_owned(), changed);
                }
            }
        }
    }
    let transition = transition(snapshot, &default.node_id, &transition_props);
    let text_properties = bind_text_properties(snapshot, set, &mut root, &dimensions);
    let component_name = legacy_component_name(set.typed_view().name().unwrap_or("Component"));
    let code = mark_node(
        &set.id,
        render_component(
            &component_name,
            &dimensions,
            &root,
            &selectors,
            transition.as_ref(),
            &text_properties,
        ),
    );
    let mut imports = BTreeSet::new();
    collect_imports(&root, &mut imports);
    Ok(Some(CodegenOutput {
        tsx: code,
        imports: imports.into_iter().collect(),
        used_tokens: BTreeSet::new(),
        diagnostics: unrepresented_variant_nodes
            .into_iter()
            .map(|node_id| Diagnostic {
                code: "DEVUP_CODEGEN_VARIANT_CHILD_FALLBACK".to_owned(),
                message: "Nesting differences in the non-default variant were replaced with the default variant structure."
                    .to_owned(),
                node_id: Some(node_id),
                severity: Some(DiagnosticSeverity::Warning),
                fidelity_impact: Some(FidelityImpact::Lossy),
                fallback: Some("default-variant-structure".to_owned()),
                ..Diagnostic::default()
            })
            .collect(),
        source_map: crate::provenance::SourceMap::empty(),
        projection_trace: crate::provenance::ProjectionTrace::default(),
        fidelity_report: crate::provenance::FidelityReport::default(),
        width_policy: None,
    }))
}

fn definitions(set: &RawNode) -> Vec<Definition> {
    set.typed_view()
        .value("componentPropertyDefinitions")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(name, definition)| {
            let boolean = match definition.get("type").and_then(Value::as_str)? {
                "VARIANT" => false,
                // A boolean property does not name variants; it switches a
                // child on and off through `componentPropertyReferences`. It
                // belongs in the props all the same, or the component cannot be
                // told to leave its icon out.
                "BOOLEAN" => true,
                _ => return None,
            };
            Some(Definition {
                name: instance_property_name(name),
                default: definition
                    .get("defaultValue")
                    .and_then(|value| {
                        value
                            .as_str()
                            .map(str::to_owned)
                            .or_else(|| value.as_bool().map(|value| value.to_string()))
                    })
                    .unwrap_or_default(),
                options: definition
                    .get("variantOptions")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
                boolean,
                source: name.clone(),
            })
        })
        .collect()
}

pub(super) fn project_tree(
    snapshot: &Snapshot,
    node: &RawNode,
    options: &CodegenOptions,
    is_render_root: bool,
) -> Result<Tree, DevupError> {
    project_tree_inner(snapshot, node, options, is_render_root, false)
}

/// As [`project_tree`], but instances become component references instead of
/// being expanded.
///
/// The variant path must keep expanding them — a component set's variants are
/// merged from the inside, and the pinned corpus records that output — so this
/// is a second entry rather than a change to the first.
pub(super) fn project_tree_keeping_instances(
    snapshot: &Snapshot,
    node: &RawNode,
    options: &CodegenOptions,
    is_render_root: bool,
) -> Result<Tree, DevupError> {
    project_tree_inner(snapshot, node, options, is_render_root, true)
}

/// The boolean component property that decides whether a node is drawn.
///
/// Figma records this on the child, as a reference back to a property the set
/// declares, which is why a set's definitions alone never reveal what a boolean
/// property actually does.
fn visible_when(view: &devup_mcp_figma::TypedNode<'_>) -> Option<String> {
    view.value("componentPropertyReferences")
        .and_then(Value::as_object)
        .and_then(|references| references.get("visible"))
        .and_then(Value::as_str)
        .map(instance_property_name)
}

/// Whether a projected node is just an asset: one shape and nothing else.
/// This is the plugin's `isAssetLeafTree`.
fn is_asset_leaf(tree: &Tree) -> bool {
    tree.children.is_empty()
        && tree.content.is_none()
        && ((tree.component == "Image" && tree.props.contains_key("src"))
            || (tree.component == "Box" && tree.props.contains_key("maskImage")))
}

/// Whether a sibling variant in the set is more than a shape. A header whose
/// mobile variant is vectors only is still a component: flattening it to an
/// `<Image>` loses the `<Header />` reference the other widths keep.
fn set_has_non_asset_variant(snapshot: &Snapshot, node: &RawNode) -> bool {
    let view = node.typed_view();
    let master = view
        .string("mainComponentId")
        .or_else(|| view.string("componentId"))
        .and_then(|id| snapshot.nodes.get(id));
    let set_id = view
        .string("mainComponentSetId")
        .or_else(|| master.and_then(|master| master.typed_view().string("parentId")));
    let Some(set) = set_id
        .and_then(|id| snapshot.nodes.get(id))
        .filter(|set| set.typed_view().node_type() == "COMPONENT_SET")
    else {
        // A screen export carries the frames, not the page the set lives on.
        // Another width's instance of the same set is the evidence left.
        return set_id.is_some_and(|set_id| {
            snapshot.nodes.values().any(|other| {
                let other_view = other.typed_view();
                other.id != node.id
                    && other_view.node_type() == "INSTANCE"
                    && other_view.string("mainComponentSetId") == Some(set_id)
                    && other_view.string("mainComponentId") != view.string("mainComponentId")
                    && style::asset_kind(snapshot, other).is_none()
            })
        });
    };
    set.typed_view()
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .filter(|variant| master.is_none_or(|master| master.id != variant.id))
        .any(|variant| style::asset_kind(snapshot, variant).is_none())
}

/// Variant properties a component set uses to describe itself rather than to
/// be told something, so an instance must not pass them.
///
/// `effect` names the interaction state a variant stands for, and the whole
/// point of it is that the *definition* folds those variants into `_hover` and
/// `_active` blocks. A call site has no state to pass — writing
/// `<Tab effect="selected" />` asks the component for a prop it does not have.
/// `viewport` is the same story for widths.
const RESERVED_VARIANT_KEYS: [&str; 2] = ["effect", "viewport"];

/// What a placed instance carries about where it sits rather than what it is.
/// The renderer moves these onto a wrapping `Box`, since a component reference
/// has nowhere to put them.
pub(super) const POSITION_PROPS: [&str; 7] = [
    "pos",
    "top",
    "right",
    "bottom",
    "left",
    "transform",
    "transformOrigin",
];

/// A component property's name as it is written in code.
///
/// This is the plugin's `sanitizePropertyName`. Figma names the first variant
/// property for the editor's language, so a file authored in Korean calls it
/// `속성 1` — and a file where someone typed it in English calls it
/// `Property 1`. Both become one identifier, and which one matters: the
/// definition the plugin emits for the same component declares that exact name.
///
/// Note what it does *not* do. The first word keeps its case, so `Property 1`
/// is `Property1` and not `property1`; folding it turned `leftIcon` into
/// `lefticon`. And only a trailing `#<digits>:<digits>` is stripped, since
/// that is Figma's own suffix rather than a character a name may not contain.
pub(super) fn instance_property_name(raw: &str) -> String {
    let stripped = raw
        .rsplit_once('#')
        .filter(|(_, suffix)| {
            suffix.split_once(':').is_some_and(|(left, right)| {
                !left.is_empty()
                    && !right.is_empty()
                    && left.bytes().all(|byte| byte.is_ascii_digit())
                    && right.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
        .map_or(raw, |(base, _)| base);

    // The Korean word takes any space after it with it, so `속성 1` is
    // `property1` rather than `property 1` waiting to be camel-cased.
    let mut normalized = String::with_capacity(stripped.len());
    let mut rest = stripped.trim();
    while let Some(at) = rest.find("속성") {
        normalized.push_str(&rest[..at]);
        normalized.push_str("property");
        rest = rest[at + "속성".len()..].trim_start();
    }
    normalized.push_str(rest);

    let mut name = String::with_capacity(normalized.len());
    let mut capitalize = false;
    for character in normalized.chars() {
        if character.is_whitespace() || character == '-' || character == '_' {
            capitalize = true;
            continue;
        }
        if capitalize {
            name.extend(character.to_uppercase());
            capitalize = false;
        } else {
            name.push(character);
        }
    }
    if name.starts_with(|character: char| character.is_ascii_digit()) {
        name.insert(0, '_');
    }
    name.retain(|character| {
        character.is_ascii_alphanumeric() || character == '_' || character == '$'
    });
    if name.is_empty() || name.bytes().all(|byte| byte.is_ascii_digit()) {
        return "variant".to_owned();
    }
    name
}

/// The component an instance refers to: the longest `COMPONENT` name the
/// instance's own name starts with, which is how `render_node` resolves it.
pub(super) fn referenced_component_name(snapshot: &Snapshot, view_name: Option<&str>) -> String {
    snapshot
        .nodes
        .values()
        .filter(|candidate| candidate.typed_view().node_type() == "COMPONENT")
        .filter_map(|candidate| candidate.typed_view().name())
        .filter(|name| view_name.is_some_and(|instance| instance.starts_with(name)))
        .max_by_key(|name| name.len())
        .map_or_else(
            || legacy_component_name(view_name.unwrap_or("Component")),
            legacy_component_name,
        )
}

fn project_tree_inner(
    snapshot: &Snapshot,
    node: &RawNode,
    options: &CodegenOptions,
    is_render_root: bool,
    keep_instances: bool,
) -> Result<Tree, DevupError> {
    let view = node.typed_view();
    if keep_instances && view.node_type() == "INSTANCE" {
        // An instance whose component is nothing but a shape is spelled out
        // rather than referenced. `<Logo />` would be a component whose entire
        // body is one masked Box, so the reference costs a file and an import
        // and says less than the Box does. The plugin does the same, and marks
        // the spot with a `{/* <Logo /> */}` comment.
        let mut props = instance_variant_props(&view);
        let mut expanded = project_tree_inner(snapshot, node, options, is_render_root, false)?;
        if (is_asset_leaf(&expanded) && !set_has_non_asset_variant(snapshot, node))
            || style::non_rendering_asset_reason(snapshot, node).is_some()
        {
            // Spelling the shape out loses which component it came from, and
            // that is the one thing a reader needs to change it in the right
            // place. The reference leaves the call it declined to write.
            let attributes = props
                .iter()
                .map(|(name, value)| format!(" {name}=\"{value}\""))
                .collect::<Vec<_>>()
                .join("");
            expanded.leading_comment = Some(format!(
                "<{}{attributes} />",
                referenced_component_name(snapshot, view.name())
            ));
            return Ok(expanded);
        }
        // The variant each width picked, collected above. These are the
        // instance's own choice, not something to merge — a component answers
        // for its own widths.

        // Where the instance sits. `<Header />` has nowhere to put this, so the
        // renderer gives it a Box; keeping the values here lets them merge
        // across widths first.
        props.extend(instance_placement(snapshot, node, options, is_render_root));
        return Ok(Tree {
            node_id: node.id.clone(),
            node_name: view.name().unwrap_or_default().to_owned(),
            source_node_ids: BTreeSet::from([node.id.clone()]),
            component: referenced_component_name(snapshot, view.name()),
            props,
            children: Vec::new(),
            content: None,
            is_component: true,
            visible_when: visible_when(&view),
            drawn_when: Vec::new(),
            leading_comment: None,
            slots: Vec::new(),
            scalar_sources: Vec::new(),
        });
    }
    let asset = super::image::asset_kind(snapshot, node, &options.asset_policies);
    let inferred_mode = view
        .value("inferredAutoLayout")
        .and_then(Value::as_object)
        .and_then(|layout| layout.get("layoutMode"))
        .and_then(Value::as_str);
    let inferred_align = |name: &str| {
        view.string(name).or_else(|| {
            view.value("inferredAutoLayout")
                .and_then(Value::as_object)
                .and_then(|layout| layout.get(name))
                .and_then(Value::as_str)
        })
    };
    let component = if asset == Some(style::AssetKind::SvgMask)
        || (asset.is_some() && style::non_rendering_asset_reason(snapshot, node).is_some())
    {
        "Box"
    } else if asset.is_some() {
        "Image"
    } else if view.node_type() == "TEXT" {
        "Text"
    } else if layout::centres_its_only_child(snapshot, node) {
        "Center"
    } else {
        match inferred_mode.or_else(|| view.string("layoutMode")) {
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
    }
    .to_owned();
    let mut props = Vec::new();
    layout::push_layout_props(
        snapshot,
        node,
        &component,
        &mut props,
        options.root_layout,
        is_render_root,
        None,
        super::image::original_image(node, &options.asset_policies),
    );
    let mut used_tokens = BTreeSet::new();
    style::push_style_props(
        snapshot,
        node,
        &component,
        asset,
        &mut props,
        &mut used_tokens,
        style::StyleOptions {
            reserve_image_content: !(is_render_root
                && options.root_layout == super::RootLayout::Embedded),
            asset_policies: &options.asset_policies,
            variable_tokens: &options.variable_tokens,
            length_tokens: &options.length_tokens,
            asset_names_per_node: options.asset_names_per_node,
            width_policy: None,
        },
    );
    text::push_text_props(
        &view,
        &options.text_style_tokens,
        &options.typography_values,
        &options.variable_tokens,
        &mut used_tokens,
        &mut props,
    );
    style::tokenize_lengths(
        snapshot,
        node,
        &mut props,
        &options.length_tokens,
        &mut used_tokens,
    );
    text::elide_tokenized_typography(&mut props, &options.typography_values, &options.length_tokens);
    if visible_when(&view).is_some() && view.bool("visible") == Some(false) {
        props.retain(|(name, _)| name != "display");
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
    let props = props
        .into_iter()
        .map(|(name, value)| match value {
            PropValue::String(value) => (name, value),
        })
        .collect();
    let children = if asset.is_some() {
        Vec::new()
    } else {
        view.child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .map(|child| project_tree_inner(snapshot, child, options, false, keep_instances))
            .collect::<Result<Vec<_>, _>>()?
    };
    let content = (view.node_type() == "TEXT").then(|| {
        text::render_text_children(
            &view,
            &options.text_style_tokens,
            &options.typography_values,
            &options.variable_tokens,
            &mut used_tokens,
            0,
        )
    });
    let tree = Tree {
        node_id: node.id.clone(),
        node_name: view.name().unwrap_or_default().to_owned(),
        source_node_ids: BTreeSet::from([node.id.clone()]),
        component,
        props,
        children,
        content,
        is_component: false,
        visible_when: visible_when(&view),
        drawn_when: Vec::new(),
        leading_comment: None,
        slots: Vec::new(),
        scalar_sources: Vec::new(),
    };
    Ok(super::image::project_tree(tree, snapshot, node, options))
}

/// The variant an instance picked, by the name a reference to its component
/// passes it as.
pub(super) fn instance_variant_props(
    view: &devup_mcp_figma::TypedNode<'_>,
) -> BTreeMap<String, String> {
    let mut props = BTreeMap::new();
    for (raw, definition) in view
        .value("componentProperties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if definition.get("type").and_then(Value::as_str) != Some("VARIANT") {
            continue;
        }
        let name = instance_property_name(raw);
        if RESERVED_VARIANT_KEYS
            .iter()
            .any(|reserved| name.eq_ignore_ascii_case(reserved))
        {
            continue;
        }
        if let Some(value) = definition.get("value").and_then(Value::as_str) {
            props.insert(name, value.to_owned());
        }
    }
    props
}

/// Where a referenced instance sits in its parent: what the `Box` wrapped
/// around the reference carries, since the reference has nowhere to put it.
pub(super) fn instance_placement(
    snapshot: &Snapshot,
    node: &RawNode,
    options: &CodegenOptions,
    is_render_root: bool,
) -> BTreeMap<String, String> {
    if node.typed_view().string("layoutPositioning") != Some("ABSOLUTE") {
        return BTreeMap::new();
    }
    let mut placement = Vec::new();
    layout::push_layout_props(
        snapshot,
        node,
        "Box",
        &mut placement,
        options.root_layout,
        is_render_root,
        None,
        super::image::original_image(node, &options.asset_policies),
    );
    placement
        .into_iter()
        .filter_map(|(name, PropValue::String(value))| {
            (POSITION_PROPS.contains(&name.as_str()) || (name == "w" && value == "100%"))
                .then_some((name, value))
        })
        .collect()
}

fn merge_source_node_ids(
    tree: &mut Tree,
    records: &[Record],
    path: &NodePath,
    unrepresented: &mut BTreeSet<String>,
    effect_name: Option<&str>,
    default_values: &BTreeMap<String, String>,
) {
    for record in records {
        let Some(source) = tree_at(&record.tree, path) else {
            // A variant that does not hold this node at all is not a shape
            // difference the merge hid: `drawn_when` states the condition it is
            // drawn under, so the absence is carried rather than lost.
            continue;
        };
        if !same_rendered_node_kind(tree, source) {
            unrepresented.insert(source.node_id.clone());
            continue;
        }
        // Differing values are usually the point of the exercise — they become
        // the maps — so a value difference is not on its own something the
        // merge papered over. The exception is a difference that only an
        // interaction state accounts for, below the root: selector blocks are
        // written for the component's own props, so a state that changes a
        // *child* has nowhere to say so and the value really is dropped.
        if !path.is_empty()
            && tree.props != source.props
            && effect_name.is_some_and(|effect| {
                record
                    .values
                    .iter()
                    .all(|(name, value)| name == effect || default_values.get(name) == Some(value))
            })
            && record.values.get(effect_name.unwrap_or_default())
                != default_values.get(effect_name.unwrap_or_default())
        {
            unrepresented.insert(source.node_id.clone());
            continue;
        }
        tree.source_node_ids.insert(source.node_id.clone());
    }
    let keys = child_keys(tree);
    for (index, child) in tree.children.iter_mut().enumerate() {
        let mut child_path = path.to_vec();
        let Some(key) = keys.get(index).cloned() else {
            continue;
        };
        child_path.push(key);
        merge_source_node_ids(
            child,
            records,
            &child_path,
            unrepresented,
            effect_name,
            default_values,
        );
    }
}

/// Whether two nodes are the same shape. What a text node *says* is a value
/// that gets folded like any other, so only whether it is a text node at all
/// counts here — comparing the words themselves reported every button whose
/// label differs by size, which is all of them.
fn same_rendered_node_kind(left: &Tree, right: &Tree) -> bool {
    left.component == right.component
        && left.content.is_some() == right.content.is_some()
        && left.children.len() == right.children.len()
}

fn find_record<'a>(
    records: &'a [Record],
    filters: &BTreeMap<String, String>,
) -> Option<&'a Record> {
    records.iter().find(|record| {
        filters
            .iter()
            .all(|(key, value)| record.values.get(key) == Some(value))
    })
}

fn merged_props(
    records: &[Record],
    definitions: &[Definition],
    dimensions: &[Definition],
    viewport: Option<&Definition>,
    effect_value: &str,
    path: Option<&NodePath>,
    defaults: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut keys = defaults.keys().cloned().collect::<BTreeSet<_>>();
    for record in records {
        if let Some(tree) = tree_at(&record.tree, path.unwrap_or_default()) {
            keys.extend(tree.props.keys().cloned());
        }
    }
    keys.into_iter()
        .filter(|prop| prop != "src" || defaults.contains_key("src"))
        .filter_map(|prop| {
            let expression = expression_for_prop(
                records,
                definitions,
                dimensions,
                viewport,
                effect_value,
                // The base props are the whole value, not a difference from
                // anything, so nothing is filtered out as unchanged.
                None,
                path.unwrap_or_default(),
                &prop,
            )?;
            if prop == "typography" {
                return Some(typography_attribute(&expression));
            }
            Some((prop.clone(), render_expression_attr(&prop, &expression)))
        })
        .collect()
}

/// Keys the plugin's prop getters write whether or not they have a value.
///
/// The plugin keys a node's shape on `Object.keys(tree.props)`, and each of
/// its getters answers with every key it knows, `null` or `undefined` where
/// there is nothing to say: `getTextAlignProps` is always `{textAlign,
/// alignContent}`, `getMinMaxProps` always all four bounds, `getLayoutProps`
/// always `w`, `h`, `aspectRatio` and `flex`. The render step drops the empty
/// ones afterwards. So these keys are on every node of their kind and never
/// tell two apart — a `<Text>` right-aligned at one width and not at the next
/// is the same shape to the plugin. Our props only ever hold what renders, so
/// the same keys are left out of the signature here to get the same answer.
const KEYS_THE_PLUGIN_ALWAYS_WRITES: &[&str] = &[
    // getTextAlignProps
    "textAlign",
    "alignContent",
    // getMinMaxProps
    "maxW",
    "maxH",
    "minW",
    "minH",
    // getLayoutProps; `boxSize` replaces `w` and `h` only when both are set and
    // equal, so it still counts
    "w",
    "h",
    "aspectRatio",
    "flex",
    // getBlendProps
    "opacity",
    "mixBlendMode",
    // getAutoLayoutProps; the component name already says whether a node is
    // laid out and which way
    "flexDir",
    "gap",
    "rowGap",
    "columnGap",
    "justifyContent",
    "alignItems",
    // getBackgroundProps; `bg` says whether there is paint
    "bgBlendMode",
    "WebkitTextFillColor",
    "bgClip",
    // getBorderProps; `outline` says whether there is a stroke
    "outlineOffset",
    // getPaddingProps writes `p` even when every side is 0
    "p",
];

/// `getPositionProps` writes all of these on an absolutely placed node,
/// whichever edges it is pinned to; only `pos` itself tells the shape.
const KEYS_THE_PLUGIN_WRITES_ON_AN_ABSOLUTE_NODE: &[&str] =
    &["left", "right", "top", "bottom", "transform"];

/// `getAutoLayoutProps` writes `display` on every laid-out node and the
/// renderer drops the `flex`; a hidden one differs by value, not by key.
const COMPONENTS_THE_PLUGIN_ALWAYS_GIVES_DISPLAY: &[&str] = &["Flex", "VStack", "Center", "Grid"];

/// What a node looks like, ignoring the values it holds.
///
/// This is the plugin's `getChildStructureSignature`. Prop *names* count but
/// their values do not, so a node that only changed a colour still matches
/// itself in another variant — less the names the plugin writes on every
/// node regardless, see [`KEYS_THE_PLUGIN_ALWAYS_WRITES`].
pub(super) fn structure_signature(tree: &Tree) -> String {
    let absolute = tree
        .props
        .get("pos")
        .is_some_and(|value| value == "absolute");
    let laid_out = COMPONENTS_THE_PLUGIN_ALWAYS_GIVES_DISPLAY.contains(&tree.component.as_str());
    let props = tree
        .props
        .keys()
        .filter(|name| !KEYS_THE_PLUGIN_ALWAYS_WRITES.contains(&name.as_str()))
        .filter(|name| {
            !(absolute && KEYS_THE_PLUGIN_WRITES_ON_AN_ABSOLUTE_NODE.contains(&name.as_str()))
        })
        .filter(|name| !(laid_out && name.as_str() == "display"))
        // `getObjectFitProps` answers for every image-filled asset, `null`
        // for the scale modes that need nothing said.
        .filter(|name| !(tree.component == "Image" && name.as_str() == "objectFit"))
        .cloned()
        .collect::<Vec<_>>()
        .join(",");
    let children = tree
        .children
        .iter()
        .map(structure_signature)
        .collect::<Vec<_>>()
        .join("|");
    let kind = if tree.is_component {
        "component"
    } else {
        "node"
    };
    let text = if tree.content.is_some() {
        "text"
    } else {
        "notext"
    };
    format!("{}::{kind}::{text}::{props}::{children}", tree.component)
}
