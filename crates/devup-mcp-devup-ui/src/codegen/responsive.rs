//! Lining up the same screen drawn at several widths.
//!
//! A responsive screen is three sibling frames in a Section, named for the
//! width they are, and the conversion wants them as one tree whose differing
//! values became arrays. That is only possible where the trees agree in shape,
//! and this module is the part that finds out: it pairs the roots up by name,
//! walks them together, and names every place they part company.
//!
//! Shape divergence is not the interesting case — it is the cost of one. Widths
//! of the same screen are meant to be the same tree three times, so a place
//! where they are not is usually a slip in the file, and the export can only
//! carry it by keeping both copies and showing each at its own widths. Saying
//! where that happened is the point of reporting it: silently keeping both
//! looks like success and hides the thing worth fixing.

use devup_mcp_figma::{RawNode, Snapshot};

/// The widths a screen may be drawn at, narrowest first — the order devup-ui's
/// responsive arrays are written in.
pub const BREAKPOINT_NAMES: [&str; 3] = ["mobile", "tablet", "desktop"];

/// One width of a screen: which breakpoint it is, and the node it starts at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakpoint {
    /// Index into [`BREAKPOINT_NAMES`], so narrowest sorts first.
    pub rank: usize,
    pub node_id: String,
}

/// A place where the widths stopped agreeing, and what to say about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divergence {
    /// The node in the widest breakpoint that has no counterpart in shape.
    pub node_id: String,
    /// How to reach it from the root, so a reader can find the same place in
    /// each width rather than only in the one being reported.
    pub path: Vec<usize>,
    pub reason: DivergenceReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DivergenceReason {
    /// The node exists at one width and not another.
    Missing,
    /// Both exist and hold a different number of children.
    ChildCount,
    /// Both exist and are different kinds of node.
    NodeType,
}

impl DivergenceReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing at another width",
            Self::ChildCount => "a different number of children",
            Self::NodeType => "a different kind of node",
        }
    }
}

fn rank_of(name: &str) -> Option<usize> {
    let name = name.trim().to_ascii_lowercase();
    BREAKPOINT_NAMES.iter().position(|known| *known == name)
}

/// The breakpoint roots this snapshot carries, narrowest first.
///
/// Empty unless there are at least two: one width is a screen, not a screen
/// that changes, and there is nothing to line up.
pub fn breakpoints(snapshot: &Snapshot) -> Vec<Breakpoint> {
    let mut found = snapshot
        .roots
        .iter()
        .filter_map(|id| {
            let node = snapshot.nodes.get(id)?;
            let rank = rank_of(node.typed_view().name()?)?;
            Some(Breakpoint {
                rank,
                node_id: id.clone(),
            })
        })
        .collect::<Vec<_>>();
    found.sort_by_key(|breakpoint| breakpoint.rank);
    found.dedup_by_key(|breakpoint| breakpoint.rank);
    if found.len() < 2 {
        return Vec::new();
    }
    found
}

fn child_ids(snapshot: &Snapshot, node_id: &str) -> Vec<String> {
    snapshot
        .nodes
        .get(node_id)
        .map(|node| {
            node.typed_view()
                .child_ids()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn node_at<'a>(snapshot: &'a Snapshot, root: &str, path: &[usize]) -> Option<&'a RawNode> {
    let mut current = root.to_owned();
    for step in path {
        current = child_ids(snapshot, &current).into_iter().nth(*step)?;
    }
    snapshot.nodes.get(&current)
}

/// Every place the widths stop agreeing in shape, in the order a reader meets
/// them. An empty result means the trees line up and their differing values can
/// become arrays.
pub fn divergences(snapshot: &Snapshot, breakpoints: &[Breakpoint]) -> Vec<Divergence> {
    let Some(widest) = breakpoints.last() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    walk(snapshot, breakpoints, widest, &mut Vec::new(), &mut found);
    found
}

fn walk(
    snapshot: &Snapshot,
    breakpoints: &[Breakpoint],
    widest: &Breakpoint,
    path: &mut Vec<usize>,
    found: &mut Vec<Divergence>,
) {
    let Some(reference) = node_at(snapshot, &widest.node_id, path) else {
        return;
    };
    let reference_children = child_ids(snapshot, &reference.id).len();

    for breakpoint in breakpoints {
        if breakpoint.rank == widest.rank {
            continue;
        }
        let reason = match node_at(snapshot, &breakpoint.node_id, path) {
            None => Some(DivergenceReason::Missing),
            Some(other) if other.node_type != reference.node_type => {
                Some(DivergenceReason::NodeType)
            }
            Some(other) if child_ids(snapshot, &other.id).len() != reference_children => {
                Some(DivergenceReason::ChildCount)
            }
            Some(_) => None,
        };
        if let Some(reason) = reason {
            found.push(Divergence {
                node_id: reference.id.clone(),
                path: path.clone(),
                reason,
            });
            // Below a shape that already parted company there is nothing to
            // compare: every descendant would be reported for the same reason,
            // burying the one place worth looking at.
            return;
        }
    }

    // An instance is not descended into. A component drawn for several widths
    // carries its own variant for each — a header is `transparent` on desktop
    // and `mobileTranspa` on mobile — so its insides differ by design, and the
    // reference keeps one `<Header />` rather than merging what is behind it.
    // Walking in here reported six shape differences that are the component
    // doing its job.
    if reference.node_type == "INSTANCE" {
        return;
    }

    for index in 0..reference_children {
        path.push(index);
        walk(snapshot, breakpoints, widest, path, found);
        path.pop();
    }
}
