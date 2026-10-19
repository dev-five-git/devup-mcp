/// Numbered cards in plain column wrappers can share one grid without losing
/// their drawn bounds. Keep the original snapshot intact for source evidence.
pub(super) struct ReadingGrid<'a> {
    pub cards: Vec<&'a RawNode>,
    pub bounds: Box4,
}

fn sequence_number(snapshot: &Snapshot, node: &RawNode, depth: usize) -> Option<u32> {
    if depth > 12 {
        return None;
    }
    let view = node.typed_view();
    if view.node_type() == "TEXT" {
        let text = view.string("characters")?.trim();
        let text = text
            .strip_prefix("STEP ")
            .or_else(|| text.strip_prefix("Step "))
            .unwrap_or(text);
        return text
            .split_whitespace()
            .next()?
            .trim_end_matches(['.', ':'])
            .parse()
            .ok();
    }
    view.child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .find_map(|child| sequence_number(snapshot, child, depth + 1))
}

pub(super) fn reading_grid<'a>(snapshot: &'a Snapshot, node: &RawNode) -> Option<ReadingGrid<'a>> {
    // A real mobile frame supplies its own layout; never override that evidence.
    let columns = node
        .typed_view()
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .filter(|child| child.typed_view().bool("visible") != Some(false))
        .collect::<Vec<_>>();
    if columns.len() < 2 {
        return None;
    }
    let mut cards = Vec::new();
    for column in columns {
        let view = column.typed_view();
        if view.string("layoutMode") != Some("VERTICAL")
            || ["fills", "strokes", "effects"].iter().any(|key| {
                view.value(key)
                    .and_then(Value::as_array)
                    .is_some_and(|paints| !paints.is_empty())
            })
            || ["paddingLeft", "paddingRight", "paddingTop", "paddingBottom"]
                .iter()
                .any(|key| view.number(key).is_some_and(|n| n != 0.0))
        {
            return None;
        }
        let group = view
            .child_ids()
            .filter_map(|id| snapshot.nodes.get(id))
            .filter(|child| child.typed_view().bool("visible") != Some(false))
            .collect::<Vec<_>>();
        if group.len() < 2 {
            return None;
        }
        for card in group {
            let number = sequence_number(snapshot, card, 0)?;
            layout_box(card)?;
            cards.push((number, card));
        }
    }
    let original = cards.iter().map(|(number, _)| *number).collect::<Vec<_>>();
    cards.sort_by_key(|(number, _)| *number);
    if cards
        .iter()
        .enumerate()
        .any(|(i, (number, _))| *number != i as u32 + 1)
        || original.windows(2).all(|pair| pair[0] < pair[1])
    {
        return None;
    }
    if super::responsive::breakpoints(snapshot).len() > 1 {
        return None;
    }
    Some(ReadingGrid {
        cards: cards.into_iter().map(|(_, card)| card).collect(),
        bounds: layout_box(node)?,
    })
}

pub(super) fn responsive_prop(props: &mut Vec<Prop>, name: &str, mobile: &str, wide: &str) {
    props.retain(|(key, _)| key != name);
    string_prop(
        props,
        name,
        serde_json::json!([mobile, null, wide]).to_string(),
    );
}

fn grid_edges(grid: &ReadingGrid<'_>, columns: bool) -> Vec<f64> {
    let mut edges = vec![if columns {
        grid.bounds.x
    } else {
        grid.bounds.y
    }];
    for card in &grid.cards {
        let bounds = layout_box(card).expect("reading grid validates card bounds");
        let (start, length) = if columns {
            (bounds.x, bounds.w)
        } else {
            (bounds.y, bounds.h)
        };
        edges.extend([start, start + length]);
    }
    edges.sort_by(f64::total_cmp);
    edges.dedup_by(|a, b| (*a - *b).abs() < 0.1);
    edges
}

pub(super) fn reading_grid_props(grid: &ReadingGrid<'_>, props: &mut Vec<Prop>) {
    props.retain(|(name, _)| {
        !matches!(
            name.as_str(),
            "w" | "h" | "gap" | "flexDir" | "alignItems" | "justifyContent"
        )
    });
    string_prop(props, "w", "100%");
    for columns in [true, false] {
        let edges = grid_edges(grid, columns);
        let tracks = edges
            .windows(2)
            .map(|pair| {
                if columns {
                    format!("minmax(0, {}fr)", format_number(pair[1] - pair[0]))
                } else {
                    px(pair[1] - pair[0])
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        responsive_prop(
            props,
            if columns {
                "gridTemplateColumns"
            } else {
                "gridTemplateRows"
            },
            if columns { "minmax(0, 1fr)" } else { "none" },
            &tracks,
        );
    }
    responsive_prop(props, "gap", "20px", "0px");
}

pub(super) fn reading_card_props(snapshot: &Snapshot, node: &RawNode, props: &mut Vec<Prop>) {
    // Find the original column parent, or a descendant's containing card.
    let mut current = node;
    for _ in 0..16 {
        let Some(parent) = snapshot
            .nodes
            .values()
            .find(|parent| parent.typed_view().child_ids().any(|id| id == current.id))
        else {
            break;
        };
        let grand = snapshot
            .nodes
            .values()
            .find(|grand| grand.typed_view().child_ids().any(|id| id == parent.id));
        if let Some(grid) = grand.and_then(|grand| reading_grid(snapshot, grand))
            && grid.cards.iter().any(|card| card.id == current.id)
        {
            string_prop(props, "maxW", "100%");
            if current.id == node.id {
                props.retain(|(name, _)| {
                    !matches!(
                        name.as_str(),
                        "pos" | "top" | "left" | "w" | "h" | "flex" | "flexBasis"
                    )
                });
                string_prop(props, "w", "100%");
                string_prop(props, "minW", "0");
                let bounds = layout_box(node).expect("reading grid validates card bounds");
                responsive_prop(props, "h", "auto", &px(bounds.h));
                for columns in [true, false] {
                    let edges = grid_edges(&grid, columns);
                    let (start, end) = if columns {
                        (bounds.x, bounds.x + bounds.w)
                    } else {
                        (bounds.y, bounds.y + bounds.h)
                    };
                    let index = |value: f64| {
                        edges
                            .iter()
                            .position(|edge| (*edge - value).abs() < 0.1)
                            .unwrap()
                            + 1
                    };
                    responsive_prop(
                        props,
                        if columns { "gridColumn" } else { "gridRow" },
                        "auto",
                        &format!("{} / {}", index(start), index(end)),
                    );
                }
            }
            break;
        }
        current = parent;
    }
}

pub(super) fn in_reading_grid(snapshot: &Snapshot, node: &RawNode) -> bool {
    let mut current = node;
    for _ in 0..16 {
        if reading_grid(snapshot, current).is_some() {
            return true;
        }
        let Some(parent) = snapshot
            .nodes
            .values()
            .find(|parent| parent.typed_view().child_ids().any(|id| id == current.id))
        else {
            break;
        };
        current = parent;
    }
    false
}
