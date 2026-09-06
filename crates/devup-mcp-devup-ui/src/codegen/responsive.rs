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
//!
//! Keeping both is not a second code path, though, which is the thing reading
//! the output does not tell you. A width that does not draw a node is handed a
//! hidden copy of one that does, and the copies go through the ordinary merge;
//! the `display` array falls out of it like any other prop. `divergences` is
//! therefore a report, not a switch.
//!
//! The rules here are ported from `devup-figma-plugin` at the commit this
//! repo's corpus pins — `src/codegen/responsive/` — and `docs/
//! responsive-merge-rules.md` says which function each came from and where
//! this repo deliberately differs.

use std::collections::{BTreeMap, BTreeSet};

use devup_mcp_figma::{DevupError, RawNode, Snapshot};

use super::{
    component::CodegenOptions,
    variant::{Tree, project_tree_keeping_instances, structure_signature},
};

/// The widths a screen may be drawn at, narrowest first — the order devup-ui's
/// responsive arrays are written in.
pub const BREAKPOINT_NAMES: [&str; 3] = ["mobile", "tablet", "desktop"];

/// How many slots a devup-ui responsive array has: `[mobile, sm, tablet, lg,
/// PC]`.
pub const SLOTS: usize = 5;

/// The width each slot runs up to, from `devup-figma-plugin`'s `BREAKPOINTS`.
/// The last slot has no bound.
const SLOT_BOUNDS: [u32; SLOTS - 1] = [480, 768, 992, 1280];

/// Which slot a frame of this width occupies.
///
/// A width is placed by how wide it is, not by what its frame is called. The
/// same three names — `mobile` / `tablet` / `desktop` — land on slots 0/2/4 in
/// one screen and 0/1/4 in another, so reading the name and assuming a slot
/// puts every value of the second screen a band too wide.
pub fn slot_of_width(width: u32) -> usize {
    SLOT_BOUNDS
        .iter()
        .position(|bound| width <= *bound)
        .unwrap_or(SLOTS - 1)
}

/// The props a disappearing value must be cleared for, from the plugin's
/// `SPECIAL_PROPS_WITH_INITIAL`. Layout, spacing and position only: a colour
/// that stops being set is left to inherit rather than reset.
const CLEARED_WHEN_DROPPED: [&str; 41] = [
    "display",
    "position",
    "pos",
    "transform",
    "w",
    "h",
    "textAlign",
    "flexDir",
    "flexWrap",
    "justify",
    "alignItems",
    "alignContent",
    "alignSelf",
    "gap",
    "rowGap",
    "columnGap",
    "flex",
    "flexGrow",
    "flexShrink",
    "flexBasis",
    "order",
    "gridTemplateColumns",
    "gridTemplateRows",
    "gridColumn",
    "gridRow",
    "gridArea",
    "top",
    "right",
    "bottom",
    "left",
    "zIndex",
    "overflow",
    "overflowX",
    "overflowY",
    "p",
    "pt",
    "pr",
    "pb",
    "pl",
    "px",
    "py",
];

/// The margin props, kept apart only because the array above is already at the
/// length rustfmt likes to fight over.
const CLEARED_WHEN_DROPPED_MARGINS: [&str; 7] = ["m", "mt", "mr", "mb", "ml", "mx", "my"];

/// A value that need not be written because it is what the prop already is,
/// from the plugin's `DEFAULT_PROPS_MAP`. Its padding and margin entries are
/// commented out upstream, so they are absent here too.
fn is_default(prop: &str, value: &str) -> bool {
    match prop {
        "alignItems" | "justifyContent" => value == "flex-start",
        "flexDir" => value == "row",
        "gap" => value == "0" || value == "0px",
        "textDecorationSkipInk"
        | "textDecorationThickness"
        | "textDecorationColor"
        | "textUnderlineOffset" => value == "auto",
        "textDecorationStyle" => value == "solid",
        _ => false,
    }
}

fn cleared_when_dropped(prop: &str) -> bool {
    CLEARED_WHEN_DROPPED.contains(&prop) || CLEARED_WHEN_DROPPED_MARGINS.contains(&prop)
}

/// What one width has to say about one prop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drawn<'a> {
    /// No width is drawn at this slot at all.
    Absent,
    /// A width is drawn here and leaves the prop unset.
    Unset,
    /// A width is drawn here and sets the prop.
    Set(&'a str),
}

/// One prop, after the widths have been compared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merged {
    /// Every width that is drawn says the same thing, so no array is needed.
    Same(Option<String>),
    /// The widths disagree, and these are the slots to write.
    Array(Vec<Option<String>>),
}

/// Lay one prop's values across the widths as a devup-ui responsive array.
///
/// A slot is written only when it changes what is in effect, because `null`
/// does not mean "no value" — it means "whatever the slot before it said". The
/// same rule is why a prop cannot simply stop: dropping it would leave the
/// narrower width's value inherited, so a layout prop that a wider width no
/// longer sets is written `"initial"`. Without it `pl={["36.5px"]}` would
/// silently keep the mobile padding at every width.
///
/// This follows `devup-figma-plugin`'s `mergePropsToResponsive` and
/// `optimizeResponsiveValue`, including two limits worth knowing. Only the
/// props in [`CLEARED_WHEN_DROPPED`] are cleared, so a `bg` that stops being
/// set still inherits. And exactly one `"initial"` is placed, at the first
/// width that exists after the last value, which is why a prop that is set,
/// dropped, then set again wider cannot be expressed.
pub fn merge_slots(prop: &str, widths: &[Drawn<'_>; SLOTS]) -> Merged {
    // One width is a screen, not a screen that changes. The reference hands
    // its props back untouched rather than wrapping each in a one-slot array.
    let mut drawn = widths.iter().filter(|width| **width != Drawn::Absent);
    if let Some(only) = drawn.next()
        && drawn.next().is_none()
    {
        return Merged::Same(match only {
            Drawn::Set(value) => Some((*value).to_owned()),
            Drawn::Absent | Drawn::Unset => None,
        });
    }

    let mut slots: Vec<Option<String>> = widths
        .iter()
        .map(|width| match width {
            Drawn::Set(value) => Some((*value).to_owned()),
            Drawn::Absent | Drawn::Unset => None,
        })
        .collect();

    if cleared_when_dropped(prop)
        && let Some(last) = slots.iter().rposition(Option::is_some)
        && last < SLOTS - 1
        // The clear has to land on a width that is actually drawn; putting it
        // on a slot no frame occupies would say nothing.
        && let Some(clear_at) = (last + 1..SLOTS).find(|slot| widths[*slot] != Drawn::Absent)
    {
        slots[clear_at] = Some("initial".to_owned());
        slots.truncate(clear_at + 1);
    }

    // A slot that repeats what is already in effect says nothing.
    let mut carried: Option<String> = None;
    for slot in &mut slots {
        let Some(value) = slot.clone() else {
            continue;
        };
        if carried.as_deref() == Some(value.as_str()) {
            *slot = None;
        } else {
            carried = Some(value);
        }
    }
    if slots
        .first()
        .and_then(Option::as_deref)
        .is_some_and(|value| is_default(prop, value))
    {
        slots[0] = None;
    }
    while slots.last().is_some_and(Option::is_none) {
        slots.pop();
    }

    match slots.len() {
        0 => Merged::Same(None),
        1 => Merged::Same(slots.swap_remove(0)),
        _ => Merged::Array(slots),
    }
}

/// One width of a screen: which breakpoint it is, and the node it starts at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakpoint {
    /// Index into [`BREAKPOINT_NAMES`]; narrowest is 0.
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

/// The breakpoint roots this snapshot carries, in the order the Section holds
/// them.
///
/// The order is kept on purpose. Where a width does not draw a node, the
/// reference gives that width a hidden copy of the node from the *first*
/// width that does — first in the Section's own layer order, which is how the
/// plugin walks it — and every value of that copy, not only its `display`,
/// lands in the array. The about hero is drawn at tablet and desktop, and its
/// picture is `w={["770px", null, "778px", null, "770px"]}`: the mobile slot
/// says 770 because desktop comes first in that Section. Sorting the roots by
/// width put tablet's 778 there instead.
///
/// Empty unless there are at least two: one width is a screen, not a screen
/// that changes, and there is nothing to line up.
pub fn breakpoints(snapshot: &Snapshot) -> Vec<Breakpoint> {
    let mut found: Vec<Breakpoint> = Vec::new();
    for id in &snapshot.roots {
        let Some(node) = snapshot.nodes.get(id) else {
            continue;
        };
        let Some(rank) = node.typed_view().name().and_then(rank_of) else {
            continue;
        };
        // Two frames with one name: the first keeps it, as in the plugin.
        if found.iter().any(|breakpoint| breakpoint.rank == rank) {
            continue;
        }
        found.push(Breakpoint {
            rank,
            node_id: id.clone(),
        });
    }
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
    let Some(widest) = breakpoints.iter().max_by_key(|breakpoint| breakpoint.rank) else {
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

// ---------------------------------------------------------------------------
// Joining the widths into one tree
// ---------------------------------------------------------------------------

/// One node's counterparts, by the slot each is drawn at.
type BySlot<T> = [Option<T>; SLOTS];

/// Where each slot's width comes in the Section's layer order; `usize::MAX`
/// for a slot no width occupies. The lowest goes first, as the plugin's
/// `firstMapValue` does: it is the width whose values fill in for a width that
/// does not draw a node, and whose element name a merged node keeps.
type Precedence = [usize; SLOTS];

/// The slot that comes first in the Section's order among those present.
fn first_slot<T>(by_slot: &BySlot<T>, precedence: &Precedence) -> Option<usize> {
    (0..SLOTS)
        .filter(|slot| by_slot[*slot].is_some())
        .min_by_key(|slot| precedence[*slot])
}

/// A node's children, grouped under the key its counterparts will be found by.
///
/// A child whose shape is unique among its siblings is keyed by that shape, so
/// renaming it in one width does not lose it. Where several siblings share a
/// shape the shape cannot tell them apart, so their name is used instead and
/// they are paired in order. Insertion order is kept, because it is what the
/// ordering below reads.
fn children_to_map(tree: &Tree) -> Vec<(String, Vec<&Tree>)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for child in &tree.children {
        *counts.entry(structure_signature(child)).or_default() += 1;
    }
    let mut grouped: Vec<(String, Vec<&Tree>)> = Vec::new();
    for child in &tree.children {
        let signature = structure_signature(child);
        let key = if counts.get(&signature) == Some(&1) {
            format!("sig:{signature}")
        } else {
            child.node_name.clone()
        };
        if let Some((_, bucket)) = grouped.iter_mut().find(|(existing, _)| *existing == key) {
            bucket.push(child);
        } else {
            grouped.push((key, vec![child]));
        }
    }
    grouped
}

/// One order for children that every width agrees with.
///
/// Each width gives an order over the children it has, and no width need have
/// them all. Taking any single width's order would drop the others' children;
/// concatenating would scramble them. So the orders are read as edges of a
/// graph and sorted topologically, with a child's average position across the
/// widths breaking ties. This is `mergeChildNameOrder`.
fn merge_child_order(per_width: &[Vec<String>]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for order in per_width {
        for name in order {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
    }
    if names.len() < 2 {
        return names;
    }

    let mut edges: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut incoming: BTreeMap<&str, usize> = names.iter().map(|name| (name.as_str(), 0)).collect();
    for order in per_width {
        for pair in order.windows(2) {
            if edges
                .entry(pair[0].as_str())
                .or_default()
                .insert(pair[1].as_str())
            {
                *incoming.entry(pair[1].as_str()).or_default() += 1;
            }
        }
    }

    let position = |name: &str| {
        let mut total = 0.0;
        let mut seen = 0.0;
        for order in per_width {
            if let Some(index) = order.iter().position(|other| other == name) {
                total += if order.len() > 1 {
                    index as f64 / (order.len() - 1) as f64
                } else {
                    0.5
                };
                seen += 1.0;
            }
        }
        if seen > 0.0 { total / seen } else { 0.5 }
    };

    let mut ready = names
        .iter()
        .filter(|name| incoming.get(name.as_str()) == Some(&0))
        .map(String::as_str)
        .collect::<Vec<_>>();
    let mut sorted: Vec<String> = Vec::new();
    while !ready.is_empty() {
        ready.sort_by(|left, right| {
            position(left)
                .partial_cmp(&position(right))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let next = ready.remove(0);
        sorted.push(next.to_owned());
        for neighbour in edges.get(next).cloned().unwrap_or_default() {
            let degree = incoming.entry(neighbour).or_insert(1);
            *degree = degree.saturating_sub(1);
            if *degree == 0 {
                ready.push(neighbour);
            }
        }
    }
    // A cycle means the widths order two children oppositely. Nothing is
    // dropped on that account; the rest keep the order they were found in.
    for name in &names {
        if !sorted.contains(name) {
            sorted.push(name.clone());
        }
    }
    sorted
}

/// Render one merged prop as the attribute text a JSX element carries.
///
/// An array is written one slot to a line, as the plugin's `JSON.stringify`
/// with an indent of two does; `render_merged` indents the continuation
/// lines to the attribute. That is also what makes the element itself
/// multi-line, whatever its prop count.
fn render_attribute(merged: &Merged) -> Option<String> {
    match merged {
        Merged::Same(None) => None,
        Merged::Same(Some(value)) => Some(format!("=\"{value}\"")),
        Merged::Array(slots) => {
            let written = slots
                .iter()
                .map(|slot| {
                    slot.as_ref()
                        .map_or_else(|| "  null".to_owned(), |value| format!("  \"{value}\""))
                })
                .collect::<Vec<_>>()
                .join(",\n");
            Some(format!("={{[\n{written}\n]}}"))
        }
    }
}

/// The `display` a devup-ui element already carries by being itself.
fn implied_display(component: &str) -> Option<&'static str> {
    match component {
        "Flex" | "VStack" | "Center" => Some("flex"),
        "Grid" => Some("grid"),
        _ => None,
    }
}

/// What `display` an element goes back to when a wider width stops hiding it.
///
/// `merge_slots` clears a dropped layout prop to `"initial"`, which is right
/// for the other forty and wrong for this one. `initial` is the value the CSS
/// specification gives a property, not the value the element has: for `w` that
/// is `auto` and for `p` it is `0`, which are what "unset" should mean, but for
/// `display` it is `inline` whatever the element is. An element's own display
/// comes from the user-agent stylesheet instead, so a `Box` shown only from
/// tablet up would come back inline rather than block.
///
/// `revert` is the keyword that would mean what is wanted, and it cannot be
/// used either: it rolls the cascade back past the author origin, which is
/// where devup-ui puts `VStack`'s own `display: flex`. So the value is written
/// out, and `display` is never cleared to `initial`.
fn natural_display(component: &str) -> &'static str {
    match component {
        "Flex" | "VStack" | "Center" => "flex",
        "Grid" => "grid",
        // `Image` is an `img`, which is inline. Everything else devup-ui draws
        // is a `div`, and `Text` is a `p`.
        "Image" => "inline",
        _ => "block",
    }
}

/// Put back the `display` the element name stands for, before any merging.
///
/// The plugin keeps `display` in a node's props and picks the element from it;
/// this projection picks the element first and drops the value. That is the
/// same thing for one width, but not across several: a width where the node is
/// hidden would leave the others with nothing to go back to, and `merge_slots`
/// would clear them to `"initial"` — which for `display` is `inline`, not
/// `flex`. The node would stop being a flex container at every width it is
/// shown. Rendering drops the value again where the element implies it, so
/// nothing is written that was not written before.
fn restore_implied_display(tree: &mut Tree) {
    if let Some(display) = implied_display(&tree.component) {
        tree.props
            .entry("display".to_owned())
            .or_insert_with(|| display.to_owned());
    }
    for child in &mut tree.children {
        restore_implied_display(child);
    }
}

/// Fold one node's counterparts into a single node whose differing values have
/// become arrays.
fn merge_trees(
    by_slot: &BySlot<Tree>,
    precedence: &Precedence,
    notes: &mut Vec<Unrepresented>,
) -> Option<Tree> {
    let first = by_slot[first_slot(by_slot, precedence)?].clone()?;

    let mut keys = BTreeSet::new();
    for tree in by_slot.iter().flatten() {
        keys.extend(tree.props.keys().cloned());
    }
    let mut props = BTreeMap::new();
    for key in keys {
        // A component's own props are not style props. devup-ui reads an array
        // only where it applies CSS, so `property1={[...]}` on `<Header />` is
        // read as the literal array and the component sees nonsense. The widest
        // width drawn wins and the rest are reported, because a screen that
        // really does want a different variant per width is asking for
        // something the target cannot express.
        if first.is_component && !is_placement_prop(&key) {
            let mut seen = Vec::new();
            for tree in by_slot.iter().flatten() {
                if let Some(value) = tree.props.get(&key) {
                    seen.push(value.clone());
                }
            }
            if let Some(widest) = seen.last().cloned() {
                if seen.iter().any(|value| *value != widest) {
                    notes.push(Unrepresented {
                        node_id: first.node_id.clone(),
                        detail: format!(
                            "{} takes {key} {} at different widths; a component prop cannot be responsive, so {widest} is used.",
                            first.component,
                            seen.join(" / ")
                        ),
                    });
                }
                props.insert(key, format!("=\"{widest}\""));
            }
            continue;
        }
        let widths: [Drawn<'_>; SLOTS] = std::array::from_fn(|slot| match &by_slot[slot] {
            None => Drawn::Absent,
            Some(tree) => tree
                .props
                .get(&key)
                .map_or(Drawn::Unset, |value| Drawn::Set(value)),
        });
        let mut merged = merge_slots(&key, &widths);
        if key == "display"
            && let Merged::Array(slots) = &mut merged
        {
            // A component reference cannot carry `display`; it lands on the
            // `Box` that will be wrapped around it below.
            let natural = if first.is_component {
                "block"
            } else {
                natural_display(&first.component)
            };
            for slot in &mut *slots {
                if slot.as_deref() == Some("initial") {
                    *slot = Some(natural.to_owned());
                }
            }
        }
        // Every width agreeing on the `display` the element already has is the
        // state this started in, so it goes back to being unwritten.
        if key == "display"
            && matches!(&merged, Merged::Same(Some(value))
                if implied_display(&first.component) == Some(value.as_str()))
        {
            continue;
        }
        if let Some(attribute) = render_attribute(&merged) {
            props.insert(key, attribute);
        }
    }

    let mut source_node_ids = BTreeSet::new();
    for tree in by_slot.iter().flatten() {
        source_node_ids.extend(tree.source_node_ids.iter().cloned());
    }

    // A component reference has nothing below it to line up: each width picks
    // its own variant, and the component answers for its own widths.
    if first.is_component {
        // Where it sits belongs to a wrapper; what variant it is belongs to the
        // reference. Only the first is merged — passing an array to a component
        // prop does nothing, so a variant that differs by width cannot be
        // expressed and the widest one drawn is kept.
        let (placement, variants): (BTreeMap<_, _>, BTreeMap<_, _>) = props
            .into_iter()
            .partition(|(name, _)| is_placement_prop(name));
        let reference = Tree {
            props: variants,
            children: Vec::new(),
            source_node_ids: source_node_ids.clone(),
            ..first.clone()
        };
        if placement.is_empty() {
            return Some(reference);
        }
        return Some(Tree {
            component: "Box".to_owned(),
            props: placement,
            children: vec![reference],
            source_node_ids,
            is_component: false,
            content: None,
            ..first
        });
    }

    Some(Tree {
        props,
        children: merge_children(by_slot, precedence, notes),
        source_node_ids,
        ..first
    })
}

fn is_placement_prop(name: &str) -> bool {
    super::variant::POSITION_PROPS.contains(&name) || name == "w" || name == "display"
}

fn merge_children(
    by_slot: &BySlot<Tree>,
    precedence: &Precedence,
    notes: &mut Vec<Unrepresented>,
) -> Vec<Tree> {
    let maps: BySlot<Vec<(String, Vec<&Tree>)>> =
        std::array::from_fn(|slot| by_slot[slot].as_ref().map(children_to_map));
    let orders = maps
        .iter()
        .flatten()
        .map(|map| map.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>())
        .collect::<Vec<_>>();

    let mut merged = Vec::new();
    for key in merge_child_order(&orders) {
        let bucket = |slot: usize| {
            maps[slot]
                .as_ref()
                .and_then(|map| map.iter().find(|(name, _)| *name == key))
                .map(|(_, children)| children)
        };
        let count = (0..SLOTS)
            .filter_map(|slot| bucket(slot).map(Vec::len))
            .max()
            .unwrap_or_default();

        for index in 0..count {
            let mut children: BySlot<Tree> = std::array::from_fn(|slot| {
                bucket(slot).and_then(|list| list.get(index).cloned().cloned())
            });
            // A width that does not draw this child is given a copy of the
            // first one that does — first in the Section's order — hidden. The
            // copies then merge like anything else: the `display` array falls
            // out of the ordinary prop merge rather than from a branch of its
            // own, and so do the copy's other values, which is why the choice
            // of width to copy shows in the output.
            if let Some(shown) =
                first_slot(&children, precedence).and_then(|slot| children[slot].clone())
            {
                for slot in 0..SLOTS {
                    if by_slot[slot].is_some() && children[slot].is_none() {
                        let mut hidden = shown.clone();
                        hidden.props.insert("display".to_owned(), "none".to_owned());
                        children[slot] = Some(hidden);
                    }
                }
            }
            merged.extend(merge_trees(&children, precedence, notes));
        }
    }
    merged
}

/// Write a merged tree as JSX, in the house style: props sorted, spread over
/// lines once there are five, and childless elements closed on themselves.
fn render_merged(tree: &Tree, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    let attributes = tree
        .props
        .iter()
        .map(|(name, attribute)| format!("{name}{attribute}"))
        .collect::<Vec<_>>();
    // Five props, or any array, and the props go one to a line — the
    // plugin's `propsToString` separator rule.
    let multiline =
        attributes.len() >= 5 || attributes.iter().any(|attribute| attribute.contains('\n'));
    let opening = if attributes.is_empty() {
        String::new()
    } else if multiline {
        let prefix = "  ".repeat(depth + 1);
        let padded = attributes
            .iter()
            .map(|attribute| attribute.replace('\n', &format!("\n{prefix}")))
            .collect::<Vec<_>>();
        format!("\n{prefix}{}", padded.join(&format!("\n{prefix}")))
    } else {
        format!(" {}", attributes.join(" "))
    };

    let mut children = tree
        .children
        .iter()
        .map(|child| render_merged(child, depth + 1))
        .collect::<Vec<_>>();
    if let Some(content) = &tree.content {
        let prefix = "  ".repeat(depth + 1);
        children.push(
            content
                .lines()
                .map(|line| format!("{prefix}{line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }

    // A shape spelled out in place of the component it came from still says
    // which component that was, or a reader has nothing to go on when the
    // change belongs upstream.
    let comment = match &tree.leading_comment {
        Some(comment) => format!("{indent}{{/* {comment} */}}\n"),
        None => String::new(),
    };
    let component = &tree.component;
    if children.is_empty() {
        if multiline {
            format!("{comment}{indent}<{component}{opening}\n{indent}/>")
        } else {
            format!("{comment}{indent}<{component}{opening} />")
        }
    } else {
        let close_open = if multiline {
            format!("\n{indent}>")
        } else {
            ">".to_owned()
        };
        format!(
            "{comment}{indent}<{component}{opening}{close_open}\n{}\n{indent}</{component}>",
            children.join("\n")
        )
    }
}

/// Something the widths asked for that a single tree cannot say.
///
/// Keeping these is the point of the exercise: an export that quietly picks one
/// width and moves on looks like a success and hides the thing worth fixing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unrepresented {
    pub node_id: String,
    pub detail: String,
}

/// A screen's widths, folded into one tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedScreen {
    /// The merged JSX, indented for a component body.
    pub tsx: String,
    /// Every element name the JSX mentions, primitives and components alike.
    pub components: BTreeSet<String>,
    /// The slot each width was placed in, narrowest first.
    pub slots: Vec<usize>,
    /// What could not be carried across, and why.
    pub unrepresented: Vec<Unrepresented>,
}

/// Everything `project_tree` can name. Anything else the merged tree mentions
/// is a component of the design system and is imported from `@/components`.
const DEVUP_PRIMITIVES: [&str; 7] = ["Box", "Center", "Flex", "Grid", "Image", "Text", "VStack"];

impl MergedScreen {
    /// The elements that come from devup-ui itself.
    pub fn primitives(&self) -> Vec<&str> {
        self.components
            .iter()
            .map(String::as_str)
            .filter(|name| DEVUP_PRIMITIVES.contains(name))
            .collect()
    }

    /// The elements that are components of this design, one file each.
    pub fn referenced_components(&self) -> Vec<&str> {
        self.components
            .iter()
            .map(String::as_str)
            .filter(|name| !DEVUP_PRIMITIVES.contains(name))
            .collect()
    }

    /// The whole file: what it imports, and the screen as a default export.
    pub fn module(&self, component_name: &str) -> String {
        let mut lines = Vec::new();
        let primitives = self.primitives();
        if !primitives.is_empty() {
            lines.push(format!(
                "import {{ {} }} from '@devup-ui/react'",
                primitives.join(", ")
            ));
        }
        lines.extend(
            self.referenced_components()
                .iter()
                .map(|name| format!("import {{ {name} }} from '@/components/{name}'")),
        );
        let imports = if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n\n", lines.join("\n"))
        };
        format!(
            "{imports}export default function {component_name}() {{\n  return (\n{}\n  )\n}}\n",
            self.tsx
        )
    }
}

fn collect_components(tree: &Tree, into: &mut BTreeSet<String>) {
    into.insert(tree.component.clone());
    for child in &tree.children {
        collect_components(child, into);
    }
}

/// Fold a screen drawn at several widths into one tree.
///
/// `None` when the snapshot holds fewer than two widths — that is a screen, not
/// a screen that changes, and it has an ordinary single-width conversion.
pub fn merge_breakpoints(
    snapshot: &Snapshot,
    options: &CodegenOptions,
) -> Result<Option<MergedScreen>, DevupError> {
    let found = breakpoints(snapshot);
    if found.len() < 2 {
        return Ok(None);
    }
    let mut by_slot: BySlot<Tree> = std::array::from_fn(|_| None);
    let mut precedence: Precedence = [usize::MAX; SLOTS];
    let mut slots = Vec::new();
    for (position, breakpoint) in found.iter().enumerate() {
        let Some(node) = snapshot.nodes.get(&breakpoint.node_id) else {
            continue;
        };
        let width = node.typed_view().number("width").unwrap_or_default();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a frame is a small positive number of pixels wide"
        )]
        let slot = slot_of_width(width.max(0.0) as u32);
        slots.push(slot);
        // Two frames in one band would overwrite each other. The first keeps
        // the slot; reporting the clash is left to the caller, which knows the
        // names, rather than silently dropping a width here.
        if by_slot[slot].is_none() {
            let mut tree = project_tree_keeping_instances(snapshot, node, options, true)?;
            restore_implied_display(&mut tree);
            by_slot[slot] = Some(tree);
            precedence[slot] = position;
        }
    }
    slots.sort_unstable();
    let mut unrepresented = Vec::new();
    Ok(
        merge_trees(&by_slot, &precedence, &mut unrepresented).map(|tree| {
            let mut components = BTreeSet::new();
            collect_components(&tree, &mut components);
            MergedScreen {
                tsx: render_merged(&tree, 2),
                components,
                slots,
                unrepresented,
            }
        }),
    )
}
