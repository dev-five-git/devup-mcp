fn bind_referenced_properties(tree: &mut Tree, dimensions: &mut Vec<Definition>) {
    if let Some(name) = &tree.visible_when
        && !dimensions.iter().any(|definition| definition.name == *name)
    {
        dimensions.push(Definition {
            name: name.clone(),
            source: name.clone(),
            default: "true".into(),
            options: Vec::new(),
            boolean: true,
        });
    }
    for child in &mut tree.children {
        bind_referenced_properties(child, dimensions);
    }
}

fn scalar_sources(
    records: &[Record],
    definitions: &[Definition],
    dimensions: &[Definition],
    viewport: Option<&Definition>,
    effect: &str,
    path: &NodePath,
) -> Vec<(String, String)> {
    if viewport.is_none() { return Vec::new(); }
    let sources = records.iter().filter_map(|record| {
        let tree = tree_at(&record.tree, path)?;
        (tree.component == "Image").then(|| tree.props.get("src").cloned()).flatten()
    }).collect::<BTreeSet<_>>();
    if sources.is_empty() {
        return Vec::new();
    }
    if sources.len() == 1 && expression_for_prop(records, definitions, dimensions, viewport, effect, None, path, "src").is_none_or(|expression| !has_responsive_value(&expression)) {
        return Vec::new();
    }
    sources.into_iter().filter_map(|source| {
        let visibility = records.iter().cloned().map(|mut record| {
            fn set(tree: &mut Tree, source: &str) {
                if tree.component == "Image" {
                    let display = if tree.props.get("src").is_some_and(|value| value == source) { "inline" } else { "none" };
                    tree.props.insert("src".into(), display.into());
                }
                for child in &mut tree.children { set(child, source); }
            }
            set(&mut record.tree, &source);
            record
        }).collect::<Vec<_>>();
        let mut display = expression_for_prop(&visibility, definitions, dimensions, viewport, effect, None, path, "src")?;
        fill_absent_display(&mut display);
        Some((render_expression_attr("src", &Expression::Literal(source)), render_expression_attr("display", &display)))
    }).collect()
}

fn has_responsive_value(expression: &Expression) -> bool {
    match expression {
        Expression::Responsive(_) => true,
        Expression::Variant(_, values) => values.iter().any(|(_, value)| has_responsive_value(value)),
        Expression::Undefined | Expression::Literal(_) | Expression::Conditional(_, _, _) => false,
    }
}

fn fill_absent_display(expression: &mut Expression) {
    match expression {
        Expression::Responsive(slots) => {
            for index in [0, slots.len().saturating_sub(1)] {
                if let Some(slot) = slots.get_mut(index) {
                    slot.get_or_insert_with(|| "none".into());
                }
            }
        }
        Expression::Variant(_, values) => {
            for (_, value) in values { fill_absent_display(value); }
        }
        Expression::Undefined => *expression = Expression::Literal("none".into()),
        Expression::Literal(_) | Expression::Conditional(_, _, _) => {}
    }
}

fn effect_selector(value: &str) -> Option<&str> {
    match value.to_ascii_lowercase().as_str() {
        "default" => Some("&"),
        "hover" | "hoevr" => Some("_hover"),
        "active" => Some("_active"),
        "focus" => Some("_focus"),
        "disabled" => Some("_disabled"),
        "selected" => Some("&[aria-selected=\"true\"]"),
        _ => None,
    }
}

fn typography_attribute(expression: &Expression) -> (String, String) {
    if let Expression::Responsive(slots) = expression {
        let values = slots.iter().flatten().collect::<BTreeSet<_>>();
        if values.len() == 1
            && let Some(value) = values.first() {
                return ("typography".into(), render_expression_attr("typography", &Expression::Literal((*value).clone())));
        }
        let slots = slots.iter().map(|slot| slot.as_ref().map_or("null".into(), |token| format!("{{ typography: {} }}", serde_json::to_string(token).unwrap_or_default()))).collect::<Vec<_>>().join(", ");
        return ("selectors".into(), format!("={{{{ \"&\": [{slots}] }}}}"));
    }
    ("typography".into(), render_expression_attr("typography", expression))
}
