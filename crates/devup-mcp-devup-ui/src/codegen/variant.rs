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
) -> Result<Option<CodegenOutput>, DevupError> {
    let set = snapshot.nodes.get(set_id).ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaNodeNotFound,
            "Variant component set was not found.",
            false,
        )
    })?;
    let definitions = definitions(set);
    if !definitions.iter().any(|definition| {
        matches!(
            definition.name.to_ascii_lowercase().as_str(),
            "effect" | "viewport"
        )
    }) {
        return Ok(None);
    }
    let records = set
        .typed_view()
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .filter_map(|node| {
            let values = node
                .typed_view()
                .value("variantProperties")?
                .as_object()?
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
                .collect::<BTreeMap<_, _>>();
            Some((node, values))
        })
        .map(|(node, values)| {
            Ok(Record {
                values,
                tree: project_tree(snapshot, node, options, true)?,
                node_id: node.id.clone(),
            })
        })
        .collect::<Result<Vec<_>, DevupError>>()?;
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
    let dimensions = definitions
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
        .find(|definition| definition.name.eq_ignore_ascii_case("effect"));
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
        effect.map(|definition| definition.name.as_str()),
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
    merge_child_props(
        &mut root,
        &records,
        &definitions,
        &dimensions,
        viewport,
        effect.map_or("default", |definition| definition.default.as_str()),
        &[],
    );

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
                selectors.insert(format!("_{effect_value}"), changed);
            }
        }
    }
    let transition = transition(snapshot, &default.node_id, &transition_props);
    let component_name = legacy_component_name(set.typed_view().name().unwrap_or("Component"));
    let code = mark_node(
        &set.id,
        render_component(
            &component_name,
            &dimensions,
            &root,
            &selectors,
            transition.as_ref(),
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
fn instance_property_name(raw: &str) -> String {
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
fn referenced_component_name(snapshot: &Snapshot, view_name: Option<&str>) -> String {
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
        let mut expanded = project_tree_inner(snapshot, node, options, is_render_root, false)?;
        if is_asset_leaf(&expanded) {
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
        if view.string("layoutPositioning") == Some("ABSOLUTE") {
            let mut placement = Vec::new();
            layout::push_layout_props(
                snapshot,
                node,
                "Box",
                &mut placement,
                options.root_layout,
                is_render_root,
            );
            for (name, PropValue::String(value)) in placement {
                if POSITION_PROPS.contains(&name.as_str()) || (name == "w" && value == "100%") {
                    props.insert(name, value);
                }
            }
        }
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
        });
    }
    let asset = style::asset_kind(snapshot, node);
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
    let component = if asset == Some(style::AssetKind::SvgMask) {
        "Box"
    } else if asset.is_some() {
        "Image"
    } else if view.node_type() == "TEXT" {
        "Text"
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
    );
    let mut used_tokens = BTreeSet::new();
    style::push_style_props(
        snapshot,
        node,
        &component,
        asset,
        &mut props,
        &mut used_tokens,
        &options.variable_tokens,
    );
    text::push_text_props(
        &view,
        &options.text_style_tokens,
        &options.variable_tokens,
        &mut used_tokens,
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
            &options.variable_tokens,
            &mut used_tokens,
            0,
        )
    });
    Ok(Tree {
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
    })
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

/// How each child is found again in another variant's tree.
///
/// Addressing a child by its position breaks the moment a variant leaves one
/// out: a `tag` button has no icon, so its text sits where the icon sat and
/// every lookup after that reads the wrong node. A child is identified by its
/// shape where that is unique among its siblings, and by its name where it is
/// not, with the occurrence to tell repeats apart. This is the plugin's
/// `treeChildrenToMap`.
fn child_keys(tree: &Tree) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for child in &tree.children {
        *counts.entry(structure_signature(child)).or_default() += 1;
    }
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    tree.children
        .iter()
        .map(|child| {
            let signature = structure_signature(child);
            let key = if counts.get(&signature) == Some(&1) {
                format!("sig:{signature}")
            } else {
                child.node_name.clone()
            };
            let occurrence = seen.entry(key.clone()).or_default();
            let at = *occurrence;
            *occurrence += 1;
            (key, at)
        })
        .collect()
}

/// Where a node sits, said in a way another variant can follow.
type NodePath = [(String, usize)];

/// The name under which a text node's own words are folded like any other
/// value. No CSS property can be called this, so it cannot collide with one.
const CONTENT_FIELD: &str = "\u{1}content";

/// What a node says for a field, whether that is a prop or its own words.
fn tree_field(tree: &Tree, field: &str) -> Option<String> {
    if field == CONTENT_FIELD {
        return tree.content.clone();
    }
    tree.props.get(field).cloned()
}

/// The conditions under which a node is drawn at all.
///
/// Not every variant holds every node: a `tag`-sized button has no icon, so the
/// merged tree — built from the default variant, which does — would draw one at
/// every size. The dimensions whose options disagree about the node's existence
/// become the condition guarding it.
///
/// Existence is judged by what the node *is* rather than by whether something
/// sits at the same index, because a variant that dropped a child shifts every
/// child after it up one place.
fn drawn_when(
    records: &[Record],
    dimensions: &[Definition],
    effect_value: &str,
    path: &NodePath,
    child: &Tree,
) -> Vec<String> {
    let mut conditions = Vec::new();
    for dimension in dimensions.iter().filter(|dimension| !dimension.boolean) {
        let parent_path = &path[..path.len().saturating_sub(1)];
        let at = |option: &String, want_node: bool| {
            records.iter().any(|record| {
                record.values.get(&dimension.source) == Some(option)
                    && record.values.iter().all(|(name, value)| {
                        !name.eq_ignore_ascii_case("effect") || value == effect_value
                    })
                    && (!want_node
                        || tree_at(&record.tree, parent_path).is_some_and(|parent| {
                            parent
                                .children
                                .iter()
                                .any(|sibling| sibling.component == child.component)
                        }))
            })
        };
        let drawn = dimension
            .options
            .iter()
            .filter(|option| at(option, false))
            .collect::<Vec<_>>();
        let holding = drawn
            .iter()
            .filter(|option| at(option, true))
            .collect::<Vec<_>>();
        if holding.is_empty() || holding.len() == drawn.len() {
            continue;
        }
        conditions.push(format!(
            "({})",
            holding
                .iter()
                .map(|option| format!("{} === \"{option}\"", dimension.name))
                .collect::<Vec<_>>()
                .join(" || ")
        ));
    }
    conditions
}

fn merge_child_props(
    tree: &mut Tree,
    records: &[Record],
    definitions: &[Definition],
    dimensions: &[Definition],
    viewport: Option<&Definition>,
    effect_value: &str,
    path: &NodePath,
) {
    let keys = child_keys(tree);
    for (index, child) in tree.children.iter_mut().enumerate() {
        let mut child_path = path.to_vec();
        let Some(key) = keys.get(index).cloned() else {
            continue;
        };
        child_path.push(key);
        child.drawn_when = drawn_when(records, dimensions, effect_value, &child_path, child);
        // A text node's own words are as much a variant as the colour around
        // them: `lg` says "buttonLg" where `tag` says "Tag".
        if child.content.is_some()
            && let Some(expression) = expression_for_prop(
                records,
                definitions,
                dimensions,
                viewport,
                effect_value,
                None,
                &child_path,
                CONTENT_FIELD,
            )
        {
            child.content = Some(match &expression {
                Expression::Literal(value) => value.clone(),
                other => format!("{{{}}}", render_expression_value(other, 0)),
            });
        }
        child.props = merged_props(
            records,
            definitions,
            dimensions,
            viewport,
            effect_value,
            Some(&child_path),
            &child.props,
        );
        merge_child_props(
            child,
            records,
            definitions,
            dimensions,
            viewport,
            effect_value,
            &child_path,
        );
    }
}

fn changed_effect_props(
    records: &[Record],
    definitions: &[Definition],
    dimensions: &[Definition],
    viewport: Option<&Definition>,
    default_effect: &str,
    effect_value: &str,
) -> BTreeMap<String, Expression> {
    let effect_name = definitions
        .iter()
        .find(|definition| definition.name.eq_ignore_ascii_case("effect"))
        .map(|definition| definition.name.clone());
    let keys = records
        .iter()
        .filter(|record| {
            effect_name.as_ref().is_some_and(|name| {
                record.values.get(name).map(String::as_str) == Some(effect_value)
            })
        })
        .flat_map(|effect_record| {
            let mut filters = effect_record.values.clone();
            if let Some(effect_name) = &effect_name {
                filters.insert(effect_name.clone(), default_effect.to_owned());
            }
            let base = find_record(records, &filters);
            effect_record
                .tree
                .props
                .iter()
                .filter(move |(key, value)| {
                    base.and_then(|record| record.tree.props.get(*key)) != Some(*value)
                })
                .map(|(key, _)| key.clone())
        })
        .collect::<BTreeSet<_>>();
    keys.into_iter()
        .filter_map(|prop| {
            let effect = expression_for_prop(
                records,
                definitions,
                dimensions,
                viewport,
                effect_value,
                // A selector block says what this state changes, so a value the
                // state leaves alone is left out of it entirely.
                Some(default_effect),
                &[],
                &prop,
            )?;
            Some((prop, effect))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn expression_for_prop(
    records: &[Record],
    definitions: &[Definition],
    dimensions: &[Definition],
    viewport: Option<&Definition>,
    effect_value: &str,
    baseline: Option<&str>,
    path: &NodePath,
    prop: &str,
) -> Option<Expression> {
    let effect_name = definitions
        .iter()
        .find(|definition| definition.name.eq_ignore_ascii_case("effect"))
        .map(|definition| definition.name.as_str());
    let dependencies = dimensions
        .iter()
        .filter(|dimension| !dimension.boolean)
        .filter(|dimension| {
            dimension_depends(
                records,
                definitions,
                effect_name,
                effect_value,
                baseline,
                path,
                prop,
                dimension,
            )
        })
        .collect::<Vec<_>>();
    if !dependencies.is_empty() {
        return nested_expression(
            records,
            definitions,
            effect_name,
            effect_value,
            path,
            prop,
            baseline,
            &dependencies,
            &BTreeMap::new(),
            viewport,
        );
    }
    viewport_expression(
        records,
        definitions,
        effect_name,
        effect_value,
        path,
        prop,
        baseline,
        &BTreeMap::new(),
        viewport,
    )
}

/// What one record says about a prop, or nothing when a baseline is given and
/// it says the same as the baseline does.
///
/// A `_hover` block is a difference, not a restatement. Asked for the whole
/// hover value it would repeat every size and colour that hover leaves alone,
/// and the one thing hover actually changes would be buried in it.
fn record_value(
    records: &[Record],
    record: &Record,
    effect_name: Option<&str>,
    baseline: Option<&str>,
    path: &NodePath,
    prop: &str,
) -> Option<String> {
    let value = tree_at(&record.tree, path).and_then(|tree| tree_field(tree, prop));
    let (Some(baseline), Some(effect_name)) = (baseline, effect_name) else {
        return value;
    };
    let mut filters = record.values.clone();
    filters.insert(effect_name.to_owned(), baseline.to_owned());
    let unchanged = find_record(records, &filters)
        .and_then(|record| tree_at(&record.tree, path))
        .and_then(|tree| tree_field(tree, prop));
    if value == unchanged { None } else { value }
}

#[allow(clippy::too_many_arguments)]
fn dimension_depends(
    records: &[Record],
    definitions: &[Definition],
    effect_name: Option<&str>,
    effect_value: &str,
    baseline: Option<&str>,
    path: &NodePath,
    prop: &str,
    dimension: &Definition,
) -> bool {
    let sets = dimension
        .options
        .iter()
        .map(|option| {
            records
                .iter()
                .filter(|record| {
                    record.values.get(&dimension.name) == Some(option)
                        && effect_name.is_none_or(|name| {
                            record.values.get(name).map(String::as_str) == Some(effect_value)
                        })
                })
                .map(|record| record_value(records, record, effect_name, baseline, path, prop))
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    let _ = definitions;
    sets.iter().skip(1).any(|set| set != &sets[0])
}

/// How deeply an expression nests, so that the shallower of two ways to write
/// the same thing can be preferred. A plain value costs nothing; a map costs
/// one plus the worst of what it holds.
fn nesting_cost(expression: &Expression) -> usize {
    match expression {
        Expression::Variant(_, values) => {
            1 + values
                .iter()
                .map(|(_, value)| nesting_cost(value))
                .max()
                .unwrap_or_default()
        }
        _ => 0,
    }
}

/// One prop's value where several dimensions decide it at once.
///
/// A value that depends on two dimensions cannot be written as one map, and
/// writing it as the default's literal loses every other combination — which is
/// what this used to do. It is written as a map inside a map instead.
///
/// Which dimension goes on the outside is not free: `{lg: {...}[varient], tag:
/// "10px"}[size]` and its transpose say the same thing at different sizes. Each
/// candidate outer dimension is costed by how deep the result nests, with the
/// number of entries as a tiebreak, and the cheapest is kept. That is the
/// plugin's `createNestedVariantProp`.
#[allow(clippy::too_many_arguments)]
fn nested_expression(
    records: &[Record],
    definitions: &[Definition],
    effect_name: Option<&str>,
    effect_value: &str,
    path: &NodePath,
    prop: &str,
    baseline: Option<&str>,
    remaining: &[&Definition],
    pinned: &BTreeMap<String, String>,
    viewport: Option<&Definition>,
) -> Option<Expression> {
    let Some((first, rest)) = remaining.split_first() else {
        return viewport_expression(
            records,
            definitions,
            effect_name,
            effect_value,
            path,
            prop,
            baseline,
            pinned,
            viewport,
        );
    };

    // Whether the set actually holds a variant for this combination. An option
    // with no value can mean two different things, and they call for opposite
    // answers: a variant that exists and leaves the prop unset is a hole, and
    // filling it would give that variant a value it refused; a combination the
    // designer never drew is not a hole at all, and leaving a map entry for it
    // only makes the map longer. So presence is asked of the record, not of the
    // prop.
    let drawn = |pinned: &BTreeMap<String, String>| {
        let filters = definitions
            .iter()
            .filter(|definition| !definition.boolean)
            .map(|definition| {
                let value = if Some(definition.name.as_str()) == effect_name {
                    effect_value
                } else if let Some(value) = pinned.get(&definition.name) {
                    value.as_str()
                } else {
                    &definition.default
                };
                (definition.name.clone(), value.to_owned())
            })
            .collect::<BTreeMap<_, _>>();
        find_record(records, &filters).is_some()
    };

    let branch = |dimension: &Definition, others: Vec<&Definition>| {
        dimension
            .options
            .iter()
            .map(|option| {
                let mut pinned = pinned.clone();
                pinned.insert(dimension.name.clone(), option.clone());
                (
                    option.clone(),
                    nested_expression(
                        records,
                        definitions,
                        effect_name,
                        effect_value,
                        path,
                        prop,
                        baseline,
                        &others,
                        &pinned,
                        viewport,
                    ),
                )
            })
            .filter_map(|(option, value)| Some((option, value?)))
            .collect::<Vec<_>>()
    };

    if rest.is_empty() {
        let values = branch(first, Vec::new());
        if values.is_empty() {
            return None;
        }
        // Every option that was drawn saying the same thing is not a choice at
        // all. Counted against the options that exist rather than all of them,
        // so a prop only one variant sets stays a choice while a value shared
        // by every variant there is collapses.
        let drawn_options = first
            .options
            .iter()
            .filter(|option| {
                let mut pinned = pinned.clone();
                pinned.insert(first.name.clone(), (*option).clone());
                drawn(&pinned)
            })
            .count();
        if values.len() == drawn_options
            && values.iter().all(|(_, value)| {
                matches!((value, &values[0].1), (Expression::Literal(left), Expression::Literal(right)) if left == right)
            })
        {
            return Some(values[0].1.clone());
        }
        // One option carrying the only value reads better as the condition it
        // is than as a map with a single entry and a hole where the rest were.
        if values.len() == 1
            && let Expression::Literal(value) = &values[0].1
        {
            return Some(Expression::Conditional(
                first.name.clone(),
                values[0].0.clone(),
                value.clone(),
            ));
        }
        return Some(Expression::Variant(first.name.clone(), values));
    }

    let mut best: Option<(usize, Expression)> = None;
    for candidate in remaining {
        let others = remaining
            .iter()
            .filter(|other| other.name != candidate.name)
            .copied()
            .collect::<Vec<_>>();
        let values = branch(candidate, others);
        if values.is_empty() {
            continue;
        }
        // The plugin's cost: every branch that still nests counts, and the
        // number of entries breaks ties at a tenth of the weight. Kept in
        // tenths so the comparison is exact rather than a float one.
        let cost = values
            .iter()
            .map(|(_, value)| nesting_cost(value) * 10)
            .sum::<usize>()
            + values.len();
        let expression = Expression::Variant(candidate.name.clone(), values);
        if best.as_ref().is_none_or(|(best_cost, _)| cost < *best_cost) {
            best = Some((cost, expression));
        }
    }
    best.map(|(_, expression)| expression)
}

#[allow(clippy::too_many_arguments)]
fn viewport_expression(
    records: &[Record],
    definitions: &[Definition],
    effect_name: Option<&str>,
    effect_value: &str,
    path: &NodePath,
    prop: &str,
    baseline: Option<&str>,
    pinned: &BTreeMap<String, String>,
    viewport: Option<&Definition>,
) -> Option<Expression> {
    let lookup = |viewport_value: Option<&str>| {
        let filters = definitions
            .iter()
            .filter(|definition| !definition.boolean)
            .map(|definition| {
                let value = if Some(definition.name.as_str()) == effect_name {
                    effect_value
                } else if definition.name.eq_ignore_ascii_case("viewport") {
                    viewport_value.unwrap_or(&definition.default)
                } else if let Some(value) = pinned.get(&definition.name) {
                    value.as_str()
                } else {
                    &definition.default
                };
                (definition.name.clone(), value.to_owned())
            })
            .collect::<BTreeMap<_, _>>();
        find_record(records, &filters)
            .and_then(|record| record_value(records, record, effect_name, baseline, path, prop))
    };
    let Some(viewport) = viewport else {
        return lookup(None).map(Expression::Literal);
    };
    let mobile = viewport
        .options
        .iter()
        .find(|value| value.eq_ignore_ascii_case("mobile"))
        .and_then(|value| lookup(Some(value)));
    let desktop = viewport
        .options
        .iter()
        .find(|value| !value.eq_ignore_ascii_case("mobile"))
        .and_then(|value| lookup(Some(value)));
    if mobile == desktop {
        mobile.or(desktop).map(Expression::Literal)
    } else {
        // A component's `viewport` variant only ever names two widths, and the
        // wider one is written at the last slot rather than the second, so the
        // value it carries starts applying at the widest breakpoint.
        Some(Expression::Responsive(vec![
            mobile, None, None, None, desktop,
        ]))
    }
}

fn tree_at<'a>(tree: &'a Tree, path: &NodePath) -> Option<&'a Tree> {
    path.iter().try_fold(tree, |current, (key, occurrence)| {
        let index = child_keys(current)
            .into_iter()
            .position(|(candidate, at)| candidate == *key && at == *occurrence)?;
        current.children.get(index)
    })
}

fn transition(
    snapshot: &Snapshot,
    default_id: &str,
    props: &BTreeSet<String>,
) -> Option<(String, String)> {
    let node = snapshot.nodes.get(default_id)?;
    let transition = node
        .typed_view()
        .value("reactions")?
        .as_array()?
        .iter()
        .find_map(|reaction| {
            reaction
                .get("transition")
                .or_else(|| reaction.get("action")?.get("transition"))
        })?;
    let duration = transition.get("duration")?.as_f64()?;
    let easing = transition
        .get("easing")
        .and_then(|easing| easing.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("EASE_IN_OUT")
        .trim_start_matches("EASE_")
        .to_ascii_lowercase()
        .replace('_', "-");
    let properties = props
        .iter()
        .map(|prop| match prop.as_str() {
            "bg" => "background",
            "boxShadow" => "box-shadow",
            value => value,
        })
        .collect::<Vec<_>>()
        .join(",");
    Some((
        format!("{}ms ease-{easing}", layout::format_number(duration)),
        properties,
    ))
}

fn render_component(
    name: &str,
    dimensions: &[Definition],
    tree: &Tree,
    selectors: &BTreeMap<String, BTreeMap<String, Expression>>,
    transition: Option<&(String, String)>,
) -> String {
    let interface = if dimensions.is_empty() {
        String::new()
    } else {
        let fields = dimensions
            .iter()
            .map(|definition| {
                if definition.boolean {
                    // Optional, because leaving it out is how a caller says the
                    // child should not be drawn.
                    return format!("  {}?: boolean", definition.name);
                }
                format!(
                    "  {}: {}",
                    definition.name,
                    definition
                        .options
                        .iter()
                        .map(|value| format!("'{value}'"))
                        .collect::<Vec<_>>()
                        .join(" | ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!("export interface {name}Props {{\n{fields}\n}}\n\n")
    };
    let signature = if dimensions.is_empty() {
        format!("export function {name}()")
    } else {
        let props = dimensions
            .iter()
            .map(|definition| definition.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!("export function {name}({{ {props} }}: {name}Props)")
    };
    let body = render_tree(tree, selectors, transition, 2);
    format!("{interface}{signature} {{\n  return (\n{body}\n  )\n}}")
}

fn render_tree(
    tree: &Tree,
    selectors: &BTreeMap<String, BTreeMap<String, Expression>>,
    transition: Option<&(String, String)>,
    depth: usize,
) -> String {
    let indent = "  ".repeat(depth);
    let child_indent = "  ".repeat(depth + 1);
    let mut rendered_props = Vec::new();
    for (selector, props) in selectors {
        let values = props
            .iter()
            .map(|(name, expression)| {
                format!(
                    "{}\"{name}\": {}",
                    "  ".repeat(depth + 2),
                    render_expression_value(expression, depth + 2)
                )
            })
            .collect::<Vec<_>>()
            .join(",\n");
        rendered_props.push(format!(
            "{child_indent}{selector}={{{{\n{values}\n{child_indent}}}}}"
        ));
    }
    for (name, expression) in &tree.props {
        rendered_props.push(format!(
            "{child_indent}{name}{}",
            indent_attribute_expression(expression, depth + 1)
        ));
    }
    if let Some((value, properties)) = transition {
        rendered_props.push(format!("{child_indent}transition=\"{value}\""));
        rendered_props.push(format!("{child_indent}transitionProperty=\"{properties}\""));
    }
    let opening = if rendered_props.is_empty() {
        format!("{indent}<{}>", tree.component)
    } else {
        format!(
            "{indent}<{}\n{}\n{indent}>",
            tree.component,
            rendered_props.join("\n")
        )
    };
    let mut children = tree
        .children
        .iter()
        .map(|child| render_tree(child, &BTreeMap::new(), None, depth + 1))
        .collect::<Vec<_>>();
    if let Some(content) = &tree.content {
        children.push(
            content
                .lines()
                .map(|line| format!("{child_indent}{line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    let rendered = if children.is_empty() {
        format!("{opening}\n{indent}</{}>", tree.component)
    } else {
        format!(
            "{opening}\n{}\n{indent}</{}>",
            children.join("\n"),
            tree.component
        )
    };
    // A node a boolean property switches on is written as that condition. The
    // brace has to wrap the whole element, so it is applied after the element
    // is rendered rather than woven into it.
    let rendered = match &tree.leading_comment {
        Some(comment) => format!("{indent}{{/* {comment} */}}\n{rendered}"),
        None => rendered,
    };
    let guards = tree
        .visible_when
        .iter()
        .cloned()
        .chain(tree.drawn_when.iter().cloned())
        .collect::<Vec<_>>();
    let rendered = if guards.is_empty() {
        rendered
    } else {
        let inner = rendered
            .lines()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "{indent}{{{} && (\n{inner}\n{indent})}}",
            guards.join(" && ")
        )
    };
    tree.source_node_ids
        .iter()
        .rev()
        .fold(rendered, |rendered, node_id| mark_node(node_id, rendered))
}

/// `typography` names a token, and devup-ui types it by the literal. Handing it
/// a map indexed at runtime widens every entry to `string` and the token is no
/// longer one, so the object is frozen: `({ lg: "buttonLg", … } as const)[size]`.
/// It is the only prop that needs it, which is how the reference writes it too.
fn needs_const(prop: &str) -> bool {
    prop == "typography"
}

fn render_expression_attr(prop: &str, expression: &Expression) -> String {
    match expression {
        Expression::Literal(value) => format!("=\"{value}\""),
        Expression::Conditional(name, option, value) => {
            format!("={{{name} === '{option}' && \"{value}\"}}")
        }
        Expression::Variant(name, _) if needs_const(prop) => {
            let rendered = render_expression_value(expression, 0);
            let object = rendered
                .strip_suffix(&format!("[{name}]"))
                .unwrap_or(&rendered);
            format!("={{({object} as const)[{name}]}}")
        }
        _ => format!("={{{}}}", render_expression_value(expression, 0)),
    }
}

fn indent_attribute_expression(expression: &str, depth: usize) -> String {
    if !expression.contains('\n') {
        return expression.to_owned();
    }
    let indent = "  ".repeat(depth);
    let child_indent = "  ".repeat(depth + 1);
    expression
        .lines()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                line.to_owned()
            } else if let Some(line) = line.strip_prefix("  ") {
                format!("{child_indent}{line}")
            } else {
                format!("{indent}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_expression_value(expression: &Expression, depth: usize) -> String {
    match expression {
        Expression::Literal(value) => format!("\"{value}\""),
        Expression::Responsive(slots) => format!(
            "[{}]",
            slots
                .iter()
                .map(|slot| slot
                    .as_ref()
                    .map_or("null".to_owned(), |value| format!("\"{value}\"")))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expression::Variant(prop, values) => {
            let indent = "  ".repeat(depth);
            let child_indent = "  ".repeat(depth + 1);
            let lines = values
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{child_indent}{key}: {}",
                        render_expression_value(value, depth + 1)
                    )
                })
                .collect::<Vec<_>>()
                .join(",\n");
            format!("{{\n{lines}\n{indent}}}[{prop}]")
        }
        Expression::Conditional(prop, option, value) => {
            format!("{prop} === '{option}' && \"{value}\"")
        }
    }
}

fn collect_imports(tree: &Tree, imports: &mut BTreeSet<String>) {
    imports.insert(tree.component.clone());
    for child in &tree.children {
        collect_imports(child, imports);
    }
}
