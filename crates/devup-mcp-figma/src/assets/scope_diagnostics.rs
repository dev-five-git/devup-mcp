/// A rejected selection describes the acquired scope, not whether Figma has
/// the node. Section indexes and flattened captures can omit real descendants.
fn asset_scope_error(snapshot: &Snapshot, requested: &str) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupInvalidInput,
        "The selected asset or nodeId is not in the acquired snapshot scope. Select its containing screen, or acquire the explicit node before exporting it.",
        false,
        json!({
            "reason": "asset-outside-acquired-scope",
            "requestedAssetId": requested,
            "acquiredRootIds": snapshot.roots,
            "acquiredNodeCount": snapshot.nodes.len(),
            "nodeExistence": "unverified",
            "recovery": {
                "tool": "devup_figma_export",
                "how": "Use the original Figma URL with the exact screen frameIds containing this node, or a URL selecting the node itself. Read assetManifest from that acquisition and retry with its assetId. A Section index is not a captured screen subtree. Do not reuse an artifact that omitted the node or invent a nearby node ID."
            }
        }),
    )
}
