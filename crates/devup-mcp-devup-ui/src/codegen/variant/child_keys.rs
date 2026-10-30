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
