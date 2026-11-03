// Text is not a responsive CSS property. Preserve the nearest captured
// instance as a unit when its direct text changes: a consumer can then bind
// each tab/button's action to the label that is actually visible at that width.
// Splitting just its Text would leave one shared interactive parent/action.
fn instance_text_branches(children: BySlot<Tree>, snapshot: &Snapshot) -> Vec<BySlot<Tree>> {
    fn words(tree: &Tree, snapshot: &Snapshot) -> Option<Vec<String>> {
        if tree.is_component || snapshot.nodes.get(&tree.node_id)?.node_type != "INSTANCE" {
            return None;
        }
        let content = tree
            .children
            .iter()
            .filter_map(|child| {
                (child.component == "Text"
                    && child
                        .props
                        .get("display")
                        .is_none_or(|value| value != "none"))
                .then_some(child.content.as_ref())
                .flatten()
                .map(|content| content.replace("<br />", ""))
            })
            .collect::<Vec<_>>();
        (!content.is_empty()).then_some(content)
    }

    let signatures: BySlot<Vec<String>> = std::array::from_fn(|slot| {
        children[slot]
            .as_ref()
            .and_then(|child| words(child, snapshot))
    });
    if children
        .iter()
        .enumerate()
        .any(|(slot, child)| child.is_some() && signatures[slot].is_none())
    {
        return vec![children];
    }
    let distinct = signatures
        .iter()
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>();
    if distinct.len() < 2 || distinct.iter().map(Vec::len).collect::<BTreeSet<_>>().len() != 1 {
        return vec![children];
    }
    distinct
        .into_iter()
        .map(|signature| {
            std::array::from_fn(|slot| {
                children[slot]
                    .clone()
                    .filter(|_| signatures[slot].as_ref() == Some(&signature))
            })
        })
        .collect()
}
