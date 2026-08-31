use std::collections::{BTreeMap, BTreeSet};

use devup_mcp_figma::{DevupError, Diagnostic, ErrorCode, RawNode, Snapshot};
use serde_json::Value;

use super::{
    component::{CodegenOptions, CodegenOutput, PropValue, legacy_component_name},
    layout, style, text,
};

#[derive(Clone, Debug)]
struct Tree {
    component: String,
    props: BTreeMap<String, String>,
    children: Vec<Tree>,
    content: Option<String>,
}

#[derive(Clone, Debug)]
struct Record {
    values: BTreeMap<String, String>,
    tree: Tree,
    node_id: String,
}

#[derive(Clone, Debug)]
struct Definition {
    name: String,
    default: String,
    options: Vec<String>,
}

#[derive(Clone, Debug)]
enum Expression {
    Literal(String),
    Responsive(Option<String>, Option<String>),
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
            "variant component set을 찾지 못했습니다.",
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
    let default_filters = definitions
        .iter()
        .map(|definition| (definition.name.clone(), definition.default.clone()))
        .collect::<BTreeMap<_, _>>();
    let default = find_record(&records, &default_filters)
        .or_else(|| records.first())
        .ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupCodegenFailed,
                "component set에 variant component가 없습니다.",
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
    let code = render_component(
        &component_name,
        &dimensions,
        &root,
        &selectors,
        transition.as_ref(),
    );
    let mut imports = BTreeSet::new();
    collect_imports(&root, &mut imports);
    Ok(Some(CodegenOutput {
        tsx: code,
        imports: imports.into_iter().collect(),
        used_tokens: BTreeSet::new(),
        diagnostics: Vec::<Diagnostic>::new(),
    }))
}

fn definitions(set: &RawNode) -> Vec<Definition> {
    set.typed_view()
        .value("componentPropertyDefinitions")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, definition)| definition.get("type").and_then(Value::as_str) == Some("VARIANT"))
        .map(|(name, definition)| Definition {
            name: name.clone(),
            default: definition
                .get("defaultValue")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            options: definition
                .get("variantOptions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        })
        .collect()
}

fn project_tree(
    snapshot: &Snapshot,
    node: &RawNode,
    options: &CodegenOptions,
    is_render_root: bool,
) -> Result<Tree, DevupError> {
    let view = node.typed_view();
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
            .map(|child| project_tree(snapshot, child, options, false))
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
        component,
        props,
        children,
        content,
    })
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
    path: Option<&[usize]>,
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
                path.unwrap_or_default(),
                &prop,
            )?;
            Some((prop, render_expression_attr(&expression)))
        })
        .collect()
}

fn merge_child_props(
    tree: &mut Tree,
    records: &[Record],
    definitions: &[Definition],
    dimensions: &[Definition],
    viewport: Option<&Definition>,
    effect_value: &str,
    path: &[usize],
) {
    for (index, child) in tree.children.iter_mut().enumerate() {
        let mut child_path = path.to_vec();
        child_path.push(index);
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
                &[],
                &prop,
            )?;
            Some((prop, effect))
        })
        .collect()
}

fn expression_for_prop(
    records: &[Record],
    definitions: &[Definition],
    dimensions: &[Definition],
    viewport: Option<&Definition>,
    effect_value: &str,
    path: &[usize],
    prop: &str,
) -> Option<Expression> {
    let effect_name = definitions
        .iter()
        .find(|definition| definition.name.eq_ignore_ascii_case("effect"))
        .map(|definition| definition.name.as_str());
    let dependencies = dimensions
        .iter()
        .filter(|dimension| {
            dimension_depends(
                records,
                definitions,
                effect_name,
                effect_value,
                path,
                prop,
                dimension,
            )
        })
        .collect::<Vec<_>>();
    if dependencies.len() == 1 {
        let dimension = dependencies[0];
        let values = dimension
            .options
            .iter()
            .map(|option| {
                (
                    option.clone(),
                    viewport_expression(
                        records,
                        definitions,
                        effect_name,
                        effect_value,
                        path,
                        prop,
                        Some((&dimension.name, option)),
                        viewport,
                    ),
                )
            })
            .collect::<Vec<_>>();
        let present = values
            .iter()
            .filter(|(_, value)| value.is_some())
            .collect::<Vec<_>>();
        if present.len() == 1
            && matches!(present[0].1, Some(Expression::Literal(_)))
            && dimension.options.len() == 2
        {
            if let Some(Expression::Literal(value)) = present[0].1.clone() {
                return Some(Expression::Conditional(
                    dimension.name.clone(),
                    present[0].0.clone(),
                    value,
                ));
            }
        }
        return Some(Expression::Variant(
            dimension.name.clone(),
            values
                .into_iter()
                .filter_map(|(key, value)| Some((key, value?)))
                .collect(),
        ));
    }
    viewport_expression(
        records,
        definitions,
        effect_name,
        effect_value,
        path,
        prop,
        None,
        viewport,
    )
}

fn dimension_depends(
    records: &[Record],
    definitions: &[Definition],
    effect_name: Option<&str>,
    effect_value: &str,
    path: &[usize],
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
                .map(|record| {
                    tree_at(&record.tree, path)
                        .and_then(|tree| tree.props.get(prop))
                        .cloned()
                })
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    let _ = definitions;
    sets.iter().skip(1).any(|set| set != &sets[0])
}

#[allow(clippy::too_many_arguments)]
fn viewport_expression(
    records: &[Record],
    definitions: &[Definition],
    effect_name: Option<&str>,
    effect_value: &str,
    path: &[usize],
    prop: &str,
    dimension: Option<(&str, &String)>,
    viewport: Option<&Definition>,
) -> Option<Expression> {
    let lookup = |viewport_value: Option<&str>| {
        let filters = definitions
            .iter()
            .map(|definition| {
                let value = if Some(definition.name.as_str()) == effect_name {
                    effect_value
                } else if definition.name.eq_ignore_ascii_case("viewport") {
                    viewport_value.unwrap_or(&definition.default)
                } else if dimension.is_some_and(|(name, _)| name == definition.name) {
                    dimension
                        .map(|(_, value)| value.as_str())
                        .unwrap_or(&definition.default)
                } else {
                    &definition.default
                };
                (definition.name.clone(), value.to_owned())
            })
            .collect::<BTreeMap<_, _>>();
        find_record(records, &filters)
            .and_then(|record| tree_at(&record.tree, path))
            .and_then(|tree| tree.props.get(prop))
            .cloned()
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
        Some(Expression::Responsive(mobile, desktop))
    }
}

fn tree_at<'a>(tree: &'a Tree, path: &[usize]) -> Option<&'a Tree> {
    path.iter()
        .try_fold(tree, |current, index| current.children.get(*index))
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
    if children.is_empty() {
        format!("{opening}\n{indent}</{}>", tree.component)
    } else {
        format!(
            "{opening}\n{}\n{indent}</{}>",
            children.join("\n"),
            tree.component
        )
    }
}

fn render_expression_attr(expression: &Expression) -> String {
    match expression {
        Expression::Literal(value) => format!("=\"{value}\""),
        Expression::Conditional(prop, option, value) => {
            format!("={{{prop} === '{option}' && \"{value}\"}}")
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
        Expression::Responsive(mobile, desktop) => format!(
            "[{}, null, null, null, {}]",
            mobile
                .as_ref()
                .map_or("null".to_owned(), |value| format!("\"{value}\"")),
            desktop
                .as_ref()
                .map_or("null".to_owned(), |value| format!("\"{value}\""))
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
