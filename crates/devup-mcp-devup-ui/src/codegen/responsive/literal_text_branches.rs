/// Literal text is content, not a responsive CSS property. Keep each captured
/// copy on its own existing Text root when confidently aligned widths disagree.
/// Instance text branches run first so an interactive label keeps its hit area.
fn literal_text_branches(children: BySlot<Tree>, snapshot: &Snapshot) -> Vec<BySlot<Tree>> {
    let signatures: BySlot<String> = std::array::from_fn(|slot| {
        let tree = children[slot].as_ref()?;
        let node = snapshot.nodes.get(&tree.node_id)?;
        if tree.is_component
            || tree.component != "Text"
            || node.node_type != "TEXT"
            || node.field_errors.contains_key("characters")
        {
            return None;
        }
        // Line-break-only differences already have a precise shared-text merge.
        Some(
            node.fields
                .get("characters")?
                .as_str()?
                .replace("\r\n", "\n")
                .replace('\r', " ")
                .replace(['\n', '\u{2028}', '\u{2029}'], ""),
        )
    });
    if children
        .iter()
        .enumerate()
        .any(|(slot, tree)| tree.is_some() && signatures[slot].is_none())
    {
        return vec![children];
    }
    let distinct = signatures
        .iter()
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>();
    if distinct.len() < 2 {
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
