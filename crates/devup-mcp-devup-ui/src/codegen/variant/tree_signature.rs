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
