

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
        child.scalar_sources = scalar_sources(records, definitions, dimensions, viewport, effect_value, &child_path);
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
        .find(|definition| definition.name.eq_ignore_ascii_case("effect") && !dimensions.iter().any(|dimension| dimension.name == definition.name))
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
        .find(|definition| definition.name.eq_ignore_ascii_case("effect") && !dimensions.iter().any(|dimension| dimension.name == definition.name))
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
    // A variant that does not draw the node has nothing to say about the
    // prop: the node is guarded so that it is never rendered there, and
    // counting its silence as a difference wrote the button icon's
    // `aspectRatio={{lg: "1", md: "1", sm: "1"}[size]}` for a value every
    // variant that draws it agrees on. A variant that draws the node and
    // leaves the prop unset is a difference, and stays one.
    let sets = dimension
        .options
        .iter()
        .filter_map(|option| {
            let set = records
                .iter()
                .filter(|record| {
                    record.values.get(&dimension.name) == Some(option)
                        && effect_name.is_none_or(|name| {
                            record.values.get(name).map(String::as_str) == Some(effect_value)
                        })
                        && tree_at(&record.tree, path).is_some()
                })
                .map(|record| record_value(records, record, effect_name, baseline, path, prop))
                .collect::<BTreeSet<_>>();
            (!set.is_empty()).then_some(set)
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
            .map(|(option, value)| (option, value.unwrap_or(Expression::Undefined)))
            .collect::<Vec<_>>()
    };

    if rest.is_empty() {
        let values = branch(first, Vec::new());
        if values.iter().all(|(_, value)| matches!(value, Expression::Undefined)) {
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
        let present = values.iter().filter(|(_, value)| !matches!(value, Expression::Undefined)).collect::<Vec<_>>();
        if present.len() == 1
            && let Expression::Literal(value) = &present[0].1
        {
            return Some(Expression::Conditional(
                first.name.clone(),
                present[0].0.clone(),
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
    text_properties: &BTreeMap<String, String>,
) -> String {
    let interface = if dimensions.is_empty() && text_properties.is_empty() {
        String::new()
    } else {
        let mut fields = dimensions
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
        for name in text_properties.keys() {
            if !fields.is_empty() {
                fields.push('\n');
            }
            fields.push_str(&format!("  {name}?: string"));
        }
        format!("export interface {name}Props {{\n{fields}\n}}\n\n")
    };
    let signature = if dimensions.is_empty() && text_properties.is_empty() {
        format!("export function {name}()")
    } else {
        let props = dimensions
            .iter()
            .map(|definition| if definition.boolean { format!("{} = {}", definition.name, definition.default) } else { definition.name.clone() })
            .chain(text_properties.iter().map(|(name, default)| format!("{name} = {default}")))
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
    if !tree.scalar_sources.is_empty() {
        let rendered = tree.scalar_sources.iter().map(|(source, display)| {
            let mut image = tree.clone();
            image.scalar_sources.clear();
            image.props.insert("src".into(), source.clone());
            image.props.insert("display".into(), display.clone());
            render_tree(&image, selectors, transition, depth)
        }).collect::<Vec<_>>().join("\n");
        let indent = "  ".repeat(depth);
        return format!("{indent}<>\n{rendered}\n{indent}</>");
    }
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
        if selector.starts_with('&') {
            let selector = serde_json::to_string(selector).unwrap_or_default();
            rendered_props.push(format!("{child_indent}selectors={{{{ {selector}: {{\n{values}\n{child_indent}}} }}}}"));
        } else {
            rendered_props.push(format!(
                "{child_indent}{selector}={{{{\n{values}\n{child_indent}}}}}"
            ));
        }
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
    // An element with nothing in it closes on itself, as the plugin's
    // `renderNode` closes it — a folded icon was being written as an open
    // and a close tag with nothing between.
    let rendered = match (rendered_props.is_empty(), children.is_empty()) {
        (true, true) => format!("{indent}<{} />", tree.component),
        (false, true) => format!(
            "{indent}<{}\n{}\n{indent}/>",
            tree.component,
            rendered_props.join("\n")
        ),
        (true, false) => format!(
            "{indent}<{}>\n{}\n{indent}</{}>",
            tree.component,
            children.join("\n"),
            tree.component
        ),
        (false, false) => format!(
            "{indent}<{}\n{}\n{indent}>\n{}\n{indent}</{}>",
            tree.component,
            rendered_props.join("\n"),
            children.join("\n"),
            tree.component
        ),
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
        Expression::Undefined => "={undefined}".into(),
        Expression::Literal(value) => format!("=\"{value}\""),
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
        Expression::Undefined => "undefined".into(),
        Expression::Literal(value) => serde_json::to_string(value).unwrap_or_default(),
        Expression::Responsive(slots) => format!(
            "[{}]",
            slots
                .iter()
                .map(|slot| slot
                    .as_ref()
                    .map_or("null".to_owned(), |value| serde_json::to_string(value).unwrap_or_default()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expression::Variant(prop, values) => {
            let indent = "  ".repeat(depth);
            let child_indent = "  ".repeat(depth + 1);
            let lines = values
                .iter()
                .map(|(key, value)| {
                    let key = if key.chars().enumerate().all(|(index, character)| {
                        character.is_ascii_alphabetic() || character == '_' || character == '$'
                            || (index > 0 && character.is_ascii_digit())
                    }) && !key.is_empty() {
                        key.clone()
                    } else {
                        serde_json::to_string(key).unwrap_or_default()
                    };
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
            let option = serde_json::to_string(option).unwrap_or_default();
            let value = serde_json::to_string(value).unwrap_or_default();
            format!("{prop} === {option} ? {value} : undefined")
        }
    }
}

fn collect_imports(tree: &Tree, imports: &mut BTreeSet<String>) {
    imports.insert(tree.component.clone());
    for child in &tree.children {
        collect_imports(child, imports);
    }
}
