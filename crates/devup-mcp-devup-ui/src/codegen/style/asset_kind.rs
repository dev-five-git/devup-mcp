#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssetKind {
    Svg,
    SvgMask,
    Png,
}

pub(crate) fn asset_kind(snapshot: &Snapshot, node: &RawNode) -> Option<AssetKind> {
    asset_kind_nested(snapshot, node, false)
}
