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

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use devup_mcp_figma::{DevupError, RawNode, Snapshot};
use serde_json::{Value, json};

use super::{
    component::{CodegenOptions, generate_component},
    variant::{
        POSITION_PROPS, Tree, instance_variant_props, project_tree_keeping_instances,
        referenced_component_name, structure_signature,
    },
};
use crate::provenance::attributes::{
    GeneratedAttribute, OwnedElement, element_tags, generated_attributes, opening_elements,
    owned_elements,
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
/// `SPECIAL_PROPS_WITH_INITIAL`, plus visibility for suppressed assets. A colour
/// that stops being set is left to inherit rather than reset.
const CLEARED_WHEN_DROPPED: [&str; 42] = [
    "display",
    "visibility",
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
/// `optimizeResponsiveValue`, with one of its limits kept: only the props in
/// [`CLEARED_WHEN_DROPPED`] are cleared, so a `bg` that stops being set still
/// inherits. The other is not kept. The plugin places exactly one
/// `"initial"`, after the last value, so a prop set, dropped, then set again
/// wider kept the narrower value at the width that dropped it; on `about` that
/// hid the one layer only the tablet draws at every width. Here every drawn
/// width that leaves the prop unset after a narrower one set it is cleared.
/// Visibility deliberately differs: every drawn, unset width restores visible
/// paint.
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
            Drawn::Unset if prop == "visibility" => Some("initial".to_owned()),
            Drawn::Absent | Drawn::Unset => None,
        })
        .collect();

    // The clear lands only on a width that is actually drawn; one on a slot no
    // frame occupies would say nothing. Repeats collapse below.
    if cleared_when_dropped(prop) {
        let mut set_before = false;
        for (slot, width) in widths.iter().enumerate() {
            match width {
                Drawn::Set(_) => set_before = true,
                Drawn::Unset if set_before => slots[slot] = Some("initial".to_owned()),
                Drawn::Unset | Drawn::Absent => {}
            }
        }
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

/// Whether roots with these names and widths could be a breakpoint family.
///
/// The question [`breakpoints`] answers, asked without a snapshot, so it can
/// be put to a Section index — which already carries every candidate's name
/// and width — before anything is collected.
///
/// It has to be answerable there. The refusal lived in projection, which runs
/// after the collection it needs: asking for `responsiveTsx` over two frames
/// of one Braillify Studio screen spent 77 seconds and ten Figma reads before
/// reporting that the snapshot had no mergeable breakpoint frames — a verdict
/// the frames' own names and widths settle in full.
///
/// Keep this in step with [`breakpoints`]. `the_predicate_agrees_with_
/// discovery` in `tests/responsive_prevalidate.rs` holds the two together, so
/// a change to either rule that forgets the other fails there rather than in
/// a 77-second export.
pub fn breakpoint_family_is_possible(frames: &[(&str, f64)]) -> bool {
    let mut ranks: Vec<usize> = Vec::new();
    for (name, _width) in frames {
        let Some(rank) = rank_of(name) else {
            continue;
        };
        if ranks.contains(&rank) {
            continue;
        }
        ranks.push(rank);
    }
    if ranks.len() >= 2 {
        return true;
    }
    width_family_is_possible(frames)
}

/// The fallback [`breakpoints_by_width`] applies, asked of names and widths
/// alone: one name across two or more distinct width bands.
///
/// Its two refusals are kept exactly, because either one loosened here would
/// let a collection start that the projection then refuses — the cost this
/// predicate exists to avoid.
fn width_family_is_possible(frames: &[(&str, f64)]) -> bool {
    let mut shared_name: Option<String> = None;
    let mut slots: Vec<usize> = Vec::new();
    for (name, width) in frames {
        let name = name.trim().to_ascii_lowercase();
        if *shared_name.get_or_insert_with(|| name.clone()) != name {
            return false;
        }
        if *width <= 0.0 {
            return false;
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a frame is a small positive number of pixels wide"
        )]
        let slot = slot_of_width(*width as u32);
        if slots.contains(&slot) {
            return false;
        }
        slots.push(slot);
    }
    slots.len() >= 2
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
        return breakpoints_by_width(snapshot);
    }
    found
}

/// The same screen drawn at several widths, for a file that does not use the
/// plugin's breakpoint names.
///
/// `slot_of_width` already places a width "by how wide it is, not by what its
/// frame is called", but discovery read the name alone, so a file naming its
/// frames anything else had no family at all: Braillify Studio draws `AUTH-01`
/// at 1920 and again at 375, both directly under one Section, and asking for
/// `responsiveTsx` over the two answered that there was nothing to merge.
///
/// Two conditions keep this from folding things the design never drew, and
/// either one failing leaves the snapshot with no family rather than a
/// guessed one:
///
/// - **One name.** `AUTH-01` and `AUTH-02` are two designs, not one design at
///   two sizes.
/// - **Distinct width bands.** The same screen appears twice at 375 — a light
///   and a dark rendering — and neither is a breakpoint of the other. A band
///   claimed twice is ambiguous, and guessing which copy wins is the failure
///   this is meant to avoid.
///
/// The roots keep the order the snapshot carries them in, for the reason
/// [`breakpoints`] documents: the first width that draws a node supplies the
/// widths that do not, and sorting changes which one that is.
fn breakpoints_by_width(snapshot: &Snapshot) -> Vec<Breakpoint> {
    let mut shared_name: Option<String> = None;
    let mut found: Vec<Breakpoint> = Vec::new();
    for id in &snapshot.roots {
        let Some(node) = snapshot.nodes.get(id) else {
            return Vec::new();
        };
        let view = node.typed_view();
        let (Some(name), Some(width)) = (view.name(), view.number("width")) else {
            return Vec::new();
        };
        let name = name.trim().to_ascii_lowercase();
        if *shared_name.get_or_insert_with(|| name.clone()) != name {
            return Vec::new();
        }
        if width <= 0.0 {
            return Vec::new();
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a frame is a small positive number of pixels wide"
        )]
        let slot = slot_of_width(width as u32);
        if found.iter().any(|breakpoint| breakpoint.rank == slot) {
            return Vec::new();
        }
        found.push(Breakpoint {
            rank: slot,
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
/// they are paired in order: the second `quote` with the second `quote`. Each
/// keeps a key of its own, so siblings drawn between them stay between them;
/// one key for all of them emitted `“ ” title` for `“ title ”`. Insertion
/// order is kept, because it is what the ordering below reads.
fn children_to_map(tree: &Tree) -> Vec<(String, Vec<&Tree>)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for child in &tree.children {
        *counts.entry(structure_signature(child)).or_default() += 1;
    }
    let mut grouped: Vec<(String, Vec<&Tree>)> = Vec::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for child in &tree.children {
        let signature = structure_signature(child);
        let mut key = if counts.get(&signature) == Some(&1) {
            format!("sig:{signature}")
        } else {
            child.node_name.clone()
        };
        // `src` is an HTML attribute, not a responsive CSS property. Keep
        // distinct sources as distinct children; the existing missing-child
        // merge supplies their visibility slots without array-valued URLs.
        if child.component == "Image"
            && let Some(source) = child.props.get("src")
        {
            key = serde_json::to_string(&(key, source)).expect("image identity is serializable");
        }
        let occurrence = seen.entry(key.clone()).or_insert(0usize);
        grouped.push((format!("{key}#{occurrence}"), vec![child]));
        *occurrence += 1;
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

/// What a layout element does by itself besides its `display`.
fn implied_layout(component: &str) -> &'static [(&'static str, &'static str)] {
    match component {
        "VStack" => &[("flexDir", "column")],
        "Center" => &[("alignItems", "center"), ("justifyContent", "center")],
        _ => &[],
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
    // The merged element is the first width's. Where another width draws it as
    // a different layout element, what each element does by itself has to be
    // written out, or the first one's would apply at every width: a `VStack`
    // root stacking a row the desktop width draws as a `Flex`.
    let restated: BySlot<Tree>;
    let components = by_slot
        .iter()
        .flatten()
        .map(|tree| tree.component.as_str())
        .collect::<BTreeSet<_>>();
    let by_slot = if components.len() > 1
        && components.iter().all(|name| LAYOUT_ELEMENTS.contains(name))
        && by_slot.iter().flatten().all(|tree| !tree.is_component)
    {
        restated = std::array::from_fn(|slot| {
            by_slot[slot].clone().map(|mut tree| {
                for (name, value) in implied_layout(&tree.component) {
                    tree.props
                        .entry((*name).to_owned())
                        .or_insert_with(|| (*value).to_owned());
                }
                tree.props
                    .entry("display".to_owned())
                    .or_insert_with(|| natural_display(&tree.component).to_owned());
                tree
            })
        });
        &restated
    } else {
        by_slot
    };
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
        if matches!(&merged, Merged::Same(Some(value))
            if implied_layout(&first.component).contains(&(key.as_str(), value.as_str())))
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
    let slots = by_slot
        .iter()
        .map(|tree| {
            tree.as_ref()
                .map(|tree| (tree.node_id.clone(), tree.content.clone()))
        })
        .collect::<Vec<_>>();

    let content = merge_content(by_slot, &first, notes);

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
            slots: slots.clone(),
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
            slots,
            ..first
        });
    }

    Some(Tree {
        props,
        children: merge_children(by_slot, precedence, notes),
        source_node_ids,
        content,
        slots,
        ..first
    })
}

/// What a text says across the widths.
///
/// Words are not a style prop and have no array to go into, so the widths
/// have to agree — with one allowance. A designer breaks a line at one width
/// and not another, and the same words then differ only by where `<br />`
/// falls; a break some widths have and others do not is written as
/// `<Box as="br" />` shown at the widths that have it. This is what the
/// plugin's `buildResponsiveTextChildren` sets out to do and does not: it
/// lines the texts up by character index, which the very break it is looking
/// for shifts, and it works on the rendered JSX of the widths, whose own
/// newlines it then rewrites as breaks.
///
/// Words that differ by more than their breaks keep the first width's, and
/// the difference is reported rather than silently overruled.
fn merge_content(
    by_slot: &BySlot<Tree>,
    first: &Tree,
    notes: &mut Vec<Unrepresented>,
) -> Option<String> {
    let first_content = first.content.as_ref()?;
    let drawn = by_slot
        .iter()
        .enumerate()
        .filter_map(|(slot, tree)| {
            tree.as_ref()
                .and_then(|tree| tree.content.as_deref())
                .map(|content| (slot, content))
        })
        .collect::<Vec<_>>();
    if drawn.iter().all(|(_, content)| *content == first_content) {
        return Some(first_content.clone());
    }

    const BREAK: &str = "<br />";
    // The words with their breaks taken out, and where each break was: the
    // offset, in characters of the words, that it sat before.
    fn without_breaks(content: &str) -> (String, Vec<usize>) {
        let mut words = String::with_capacity(content.len());
        let mut breaks = Vec::new();
        let mut rest = content;
        let mut offset = 0;
        while !rest.is_empty() {
            if let Some(after) = rest.strip_prefix(BREAK) {
                breaks.push(offset);
                rest = after;
                continue;
            }
            let character = rest.chars().next().expect("non-empty");
            words.push(character);
            offset += 1;
            rest = &rest[character.len_utf8()..];
        }
        (words, breaks)
    }
    let stripped = drawn
        .iter()
        .map(|(slot, content)| (*slot, without_breaks(content)))
        .collect::<Vec<_>>();
    let (words, _) = &stripped[0].1;
    if stripped.iter().any(|(_, (other, _))| other != words) {
        notes.push(Unrepresented {
            node_id: first.node_id.clone(),
            detail: format!(
                "{} reads differently at different widths; the first width's text is used.",
                first.component
            ),
        });
        return Some(first_content.clone());
    }

    let breaks_at = |slot: usize, offset: usize| {
        stripped
            .iter()
            .find(|(other, _)| *other == slot)
            .map_or(0, |(_, (_, breaks))| {
                breaks.iter().filter(|at| **at == offset).count()
            })
    };
    let characters = words.chars().collect::<Vec<_>>();
    let mut merged = String::with_capacity(first_content.len());
    for offset in 0..=characters.len() {
        let most = stripped
            .iter()
            .map(|(slot, _)| breaks_at(*slot, offset))
            .max()
            .unwrap_or_default();
        for nth in 1..=most {
            let shown: BySlot<bool> = std::array::from_fn(|slot| {
                by_slot[slot]
                    .as_ref()
                    .map(|_| breaks_at(slot, offset) >= nth)
            });
            if shown.iter().flatten().all(|shown| *shown) {
                merged.push_str(BREAK);
                continue;
            }
            // Shown where the width breaks, `none` where it does not; a slot
            // repeating the one in effect says nothing, and the first slot
            // shown needs no value at all — a break is shown by default.
            let mut slots: Vec<Option<String>> = shown
                .iter()
                .map(|shown| shown.map(|shown| if shown { "initial" } else { "none" }.to_owned()))
                .collect();
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
            if slots.first().and_then(Option::as_deref) == Some("initial") {
                slots[0] = None;
            }
            while slots.last().is_some_and(Option::is_none) {
                slots.pop();
            }
            let written = slots
                .iter()
                .map(|slot| {
                    slot.as_ref()
                        .map_or_else(|| "null".to_owned(), |value| format!("\"{value}\""))
                })
                .collect::<Vec<_>>()
                .join(", ");
            merged.push_str(&format!("<Box as=\"br\" display={{[{written}]}} />"));
        }
        if let Some(character) = characters.get(offset) {
            merged.push(*character);
        }
    }
    Some(merged)
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
            // A layer hidden at every width that has it is drawn at none, and
            // `tsx` leaves it out — with its asset references, which a hidden
            // layer has none of to deliver. A copy hidden everywhere would
            // say nothing but `display: none`.
            if children.iter().flatten().all(|child| {
                child
                    .props
                    .get("display")
                    .is_some_and(|value| value == "none")
            }) {
                continue;
            }
            if (0..SLOTS).any(|slot| by_slot[slot].is_some() && children[slot].is_none()) {
                for child in children.iter_mut().flatten() {
                    if child.component == "Image" {
                        // A present source must restore visibility even when
                        // it is absent on both sides of this drawn slot.
                        child
                            .props
                            .entry("display".to_owned())
                            .or_insert_with(|| natural_display("Image").to_owned());
                    }
                }
            }
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
    /// Every attribute of `tsx`, as `attribute_contract` lists one, with
    /// whether each width's own verified TSX writes what it resolves to at
    /// that width's slot.
    pub contract: Vec<Value>,
    /// Read at each width's slot, the module writes that width's own TSX:
    /// the same elements with the same values, nothing added, nothing
    /// missing, and the same text.
    pub verified: bool,
    /// Where a width and the module part company, when they do.
    pub gaps: Vec<Value>,
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
    let mut roots: BySlot<String> = std::array::from_fn(|_| None);
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
            roots[slot] = Some(breakpoint.node_id.clone());
            precedence[slot] = position;
        }
    }
    slots.sort_unstable();
    let mut unrepresented = Vec::new();
    let Some(tree) = merge_trees(&by_slot, &precedence, &mut unrepresented) else {
        return Ok(None);
    };
    let mut components = BTreeSet::new();
    collect_components(&tree, &mut components);
    let tsx = render_merged(&tree, 2);
    let (contract, gaps) = verify_merge(snapshot, options, &roots, &tree, &tsx);
    Ok(Some(MergedScreen {
        verified: gaps.is_empty() && contract.iter().all(|entry| entry["verified"] == true),
        tsx,
        components,
        slots,
        unrepresented,
        contract,
        gaps,
    }))
}

/// One opening element of the merged module, in document order.
struct Placed<'a> {
    tree: &'a Tree,
    /// Inside the tree's text rather than the tree's own element.
    in_text: bool,
    /// Whether the element is drawn at each slot.
    shown: [bool; SLOTS],
    parent_shown: [bool; SLOTS],
    /// The element it sits in, by its place in the same order.
    parent: Option<usize>,
}

/// The merged elements in the order `render_merged` writes them: an element,
/// then its children, then whatever its text holds.
fn place<'a>(
    tree: &'a Tree,
    parent: Option<usize>,
    parent_shown: [bool; SLOTS],
    placed: &mut Vec<Placed<'a>>,
) {
    let display = tree
        .props
        .get("display")
        .and_then(|value| slot_values(value));
    let shown = std::array::from_fn(|slot| {
        parent_shown[slot]
            && display
                .as_deref()
                .and_then(|values| effective(values, slot))
                != Some("none")
    });
    let index = placed.len();
    placed.push(Placed {
        tree,
        in_text: false,
        shown,
        parent_shown,
        parent,
    });
    for child in &tree.children {
        place(child, Some(index), shown, placed);
    }
    if let Some(content) = &tree.content {
        for _ in opening_elements(&format!("<>{content}</>")) {
            placed.push(Placed {
                tree,
                in_text: true,
                shown,
                parent_shown: shown,
                parent: Some(index),
            });
        }
    }
}

/// A merged value as written after its name: `="v"` for every width, or
/// `={[...]}` with one entry per slot, `null` keeping the narrower one's.
fn slot_values(written: &str) -> Option<Vec<Option<String>>> {
    if let Some(value) = written
        .strip_prefix("=\"")
        .and_then(|rest| rest.strip_suffix('"'))
    {
        return Some(vec![Some(value.to_owned())]);
    }
    let mut rest = written
        .strip_prefix("={[")?
        .strip_suffix("]}")?
        .trim_start();
    let mut values = Vec::new();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("null") {
            values.push(None);
            rest = after;
        } else {
            let after = rest.strip_prefix('"')?;
            let end = after.find('"')?;
            values.push(Some(after[..end].to_owned()));
            rest = &after[end + 1..];
        }
        rest = rest.trim_start();
        rest = rest.strip_prefix(',').unwrap_or(rest).trim_start();
    }
    Some(values)
}

/// What devup-ui applies at a slot: the last value at or below it.
fn effective(values: &[Option<String>], slot: usize) -> Option<&str> {
    values
        .iter()
        .take(slot + 1)
        .rev()
        .find_map(Option::as_deref)
}

/// A value as the element ends up with it, so that two spellings CSS draws
/// the same compare equal and nothing else does. `None` is the property's
/// CSS initial value. An element that writes nothing has what its own class
/// sets — `Center` centres, `VStack` stacks — and `initial` undoes that
/// class, except for `display`, whose initial value is `inline` whatever the
/// element is. An inherited `initial` is settled by the caller, which knows
/// the ancestors.
fn settled(prop: &str, value: Option<&str>, tag: &str) -> Option<String> {
    let value = match value {
        None if prop == "display" => return Some(natural_display(tag).to_owned()),
        None => implied_layout(tag)
            .iter()
            .find(|(name, _)| *name == prop)
            .map(|(_, value)| *value),
        Some("initial") if prop == "display" => return Some("inline".to_owned()),
        Some("initial") if INHERITED.contains(&prop) => Some("initial"),
        Some("initial") => None,
        value => value,
    };
    match (prop, value) {
        ("flexDir", Some("row"))
        | ("gap", Some("0" | "0px"))
        | (
            "textDecorationSkipInk" | "textDecorationThickness" | "textUnderlineOffset",
            Some("auto"),
        )
        | ("textDecorationStyle", Some("solid")) => None,
        (_, value) => value.map(str::to_owned),
    }
}

/// The devup-ui elements that are one `div` laid out by their props, so one
/// stands for another wherever the props say what the other does by itself.
const LAYOUT_ELEMENTS: [&str; 5] = ["Box", "Flex", "VStack", "Center", "Grid"];

/// The props in [`CLEARED_WHEN_DROPPED`] an element inherits when it does
/// not set them, where `initial` is not the same as leaving them unset.
const INHERITED: [&str; 2] = ["textAlign", "visibility"];

/// The value a generated attribute writes, as JSX reads it.
fn written(attribute: &GeneratedAttribute) -> String {
    attribute
        .source
        .strip_prefix(attribute.name.as_str())
        .and_then(|rest| rest.strip_prefix("=\""))
        .and_then(|rest| rest.strip_suffix('"'))
        .map_or_else(
            || attribute.source.clone(),
            |value| {
                value
                    .replace("&quot;", "\"")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&amp;", "&")
            },
        )
}

/// A text read at one slot: each break written for some widths becomes a
/// `<br />` where it is shown and nothing where it is not.
fn content_at(content: &str, slot: usize) -> String {
    const OPEN: &str = "<Box as=\"br\" display={[";
    const CLOSE: &str = "]} />";
    let mut read = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(at) = rest.find(OPEN) {
        read.push_str(&rest[..at]);
        let after = &rest[at + OPEN.len()..];
        let Some(end) = after.find(CLOSE) else {
            read.push_str(&rest[at..]);
            return read;
        };
        let values = slot_values(&format!("={{[{}]}}", &after[..end]));
        if values.as_deref().and_then(|values| effective(values, slot)) != Some("none") {
            read.push_str("<br />");
        }
        rest = &after[end + CLOSE.len()..];
    }
    read.push_str(rest);
    read
}

fn descends(snapshot: &Snapshot, node: &str, ancestor: &str) -> bool {
    let mut current = Some(node);
    for _ in 0..1024 {
        let Some(id) = current else {
            return false;
        };
        if id == ancestor {
            return true;
        }
        current = snapshot
            .nodes
            .get(id)
            .and_then(|node| node.typed_view().string("parentId"));
    }
    false
}

/// The `Box` a component reference is wrapped in to carry where it sits.
fn wraps_a_reference(tree: &Tree) -> bool {
    !tree.is_component
        && tree.children.len() == 1
        && tree.children[0].is_component
        && tree.children[0].node_id == tree.node_id
}

/// The merged module against each width's own TSX.
///
/// Each width is converted on its own, instances spelled out as in `tsx`,
/// and its elements are taken in order, by the node they were drawn from.
/// The module is then read at that width's slot — every array resolved the
/// way devup-ui resolves it, elements hidden there skipped — and the two
/// must agree: the same elements, each with the same element name and the
/// same settled values, every value one the width's own contract verifies,
/// the same text, and no element of the width left over. A component
/// reference stands for the instance's whole subtree; the instance's own
/// variant and placement are what is compared. An element no collected
/// width draws is read at none of them, so its values are not verified.
///
/// This reads the code; it does not render it. It is what makes the merged
/// attributes as verified as the per-width ones, and no more.
fn verify_merge(
    snapshot: &Snapshot,
    options: &CodegenOptions,
    roots: &BySlot<String>,
    tree: &Tree,
    tsx: &str,
) -> (Vec<Value>, Vec<Value>) {
    let attributes = generated_attributes(tsx);
    let tags = element_tags(tsx);
    let mut placed = Vec::new();
    place(tree, None, [true; SLOTS], &mut placed);
    let mut ok = vec![true; attributes.len()];
    let mut gaps = Vec::new();
    if placed.len() == tags.len() {
        let mut by_element = vec![Vec::new(); placed.len()];
        for (index, attribute) in attributes.iter().enumerate() {
            by_element[attribute.element].push(index);
        }
        let reference = CodegenOptions {
            inline_instances: true,
            ..options.clone()
        };
        for (slot, root) in roots.iter().enumerate() {
            let Some(root) = root else {
                continue;
            };
            match generate_component(snapshot, root, &reference) {
                Ok(output) => {
                    let check = WidthCheck {
                        snapshot,
                        slot,
                        placed: &placed,
                        tags: &tags,
                        attributes: &attributes,
                        by_element: &by_element,
                    };
                    check.run(&owned_elements(&output), &mut ok, &mut gaps);
                }
                Err(error) => {
                    ok.fill(false);
                    gaps.push(json!({"reason":"width-not-converted","slot":slot,
                        "widthNodeId":root,"detail":error.message}));
                }
            }
        }
        for (index, element) in placed.iter().enumerate() {
            if !(0..SLOTS).any(|slot| roots[slot].is_some() && element.shown[slot]) {
                for attribute in &by_element[index] {
                    if attributes[*attribute].name != "display" {
                        ok[*attribute] = false;
                    }
                }
                gaps.push(json!({"reason":"drawn-at-no-width","element":index,
                    "nodeId":element.tree.node_id,"tag":tags[index]}));
            }
        }
    } else {
        ok.fill(false);
        gaps.push(
            json!({"reason":"elements-not-aligned","placed":placed.len(),"written":tags.len()}),
        );
    }
    let aligned = placed.len() == tags.len();
    let contract = attributes
        .iter()
        .zip(&ok)
        .map(|(attribute, verified)| {
            json!({"elementIndex":attribute.element,"tag":attribute.tag,
                "nodeId":aligned.then(|| placed[attribute.element].tree.node_id.as_str()),
                "generatedProperty":attribute.source,"verified":verified})
        })
        .collect();
    (contract, gaps)
}

struct WidthCheck<'a> {
    snapshot: &'a Snapshot,
    slot: usize,
    placed: &'a [Placed<'a>],
    tags: &'a [String],
    attributes: &'a [GeneratedAttribute],
    by_element: &'a [Vec<usize>],
}

impl WidthCheck<'_> {
    fn run(&self, width: &[OwnedElement], ok: &mut [bool], gaps: &mut Vec<Value>) {
        let slot = self.slot;
        let mut queue: BTreeMap<&str, VecDeque<usize>> = BTreeMap::new();
        for (index, element) in width.iter().enumerate() {
            if let Some(node) = element.node_id.as_deref() {
                queue.entry(node).or_default().push_back(index);
            }
        }
        let mut matched = vec![false; width.len()];
        // The width's elements are taken in the module's order, so each one
        // taken must come after the last: a node drawn in another place, or
        // siblings the widths order differently, is not the same code.
        let mut last: Option<usize> = None;
        let mut text_differs = BTreeSet::new();
        let fail = |indexes: &[usize], ok: &mut [bool], gap: Value, gaps: &mut Vec<Value>| {
            for index in indexes {
                ok[*index] = false;
            }
            gaps.push(gap);
        };
        for (index, element) in self.placed.iter().enumerate() {
            let own = &self.by_element[index];
            let tag = self.tags[index].as_str();
            let Some(Some((node, text))) = element.tree.slots.get(slot) else {
                if element.shown[slot] {
                    fail(
                        own,
                        ok,
                        json!({"reason":"no-source-at-slot","slot":slot,
                        "element":index,"tag":tag}),
                        gaps,
                    );
                }
                continue;
            };
            let gap = |reason: &str| json!({"reason":reason,"slot":slot,"widthNodeId":node,"element":index,"tag":tag});
            if !element.shown[slot] {
                if element.parent_shown[slot]
                    && !element.in_text
                    && queue
                        .get(node.as_str())
                        .is_some_and(|waiting| waiting.iter().any(|w| !matched[*w]))
                {
                    let display = own
                        .iter()
                        .copied()
                        .filter(|a| self.attributes[*a].name == "display")
                        .collect::<Vec<_>>();
                    fail(&display, ok, gap("hidden-but-drawn-at-width"), gaps);
                }
                continue;
            }
            let waiting = queue.entry(node.as_str()).or_default();
            if element.in_text {
                if tag == "Box" {
                    if text_differs.contains(&(element.tree as *const Tree)) {
                        for a in own {
                            ok[*a] = false;
                        }
                    }
                    let display = own
                        .iter()
                        .map(|a| &self.attributes[*a])
                        .find(|attribute| attribute.name == "display")
                        .and_then(|attribute| slot_values(&attribute.source["display".len()..]));
                    if display
                        .as_deref()
                        .and_then(|values| effective(values, slot))
                        != Some("none")
                    {
                        match waiting.pop_front() {
                            Some(w)
                                if width[w].tag == "br"
                                    && width[w].attributes.is_empty()
                                    && last.is_none_or(|last| w > last) =>
                            {
                                matched[w] = true;
                                last = Some(w);
                            }
                            other => {
                                if let Some(w) = other {
                                    matched[w] = true;
                                }
                                fail(own, ok, gap("break-differs-at-width"), gaps);
                            }
                        }
                    }
                    continue;
                }
                let Some(w) = waiting.pop_front() else {
                    fail(own, ok, gap("element-only-in-merge"), gaps);
                    continue;
                };
                matched[w] = true;
                if !last.is_none_or(|last| w > last) {
                    fail(own, ok, gap("order-differs-at-width"), gaps);
                }
                last = Some(w);
                let mine = own
                    .iter()
                    .map(|a| self.attributes[*a].source.as_str())
                    .collect::<BTreeSet<_>>();
                let theirs = width[w]
                    .attributes
                    .iter()
                    .map(|(attribute, _)| attribute.source.as_str())
                    .collect::<BTreeSet<_>>();
                if width[w].tag != tag
                    || mine != theirs
                    || width[w].attributes.iter().any(|(_, verified)| !verified)
                {
                    fail(own, ok, gap("text-element-differs-at-width"), gaps);
                }
                continue;
            }
            if element.tree.is_component {
                let drawn = self.reference(element, node, own, width, &mut matched, ok, gaps);
                if let (Some(first), Some(end)) = (drawn.first(), drawn.last()) {
                    if !last.is_none_or(|last| *first > last) {
                        fail(own, ok, gap("order-differs-at-width"), gaps);
                    }
                    last = Some(*end);
                }
                continue;
            }
            if wraps_a_reference(element.tree) {
                let Some(&w) = waiting.front() else {
                    fail(own, ok, gap("element-only-in-merge"), gaps);
                    continue;
                };
                self.compare(own, &width[w], tag, true, node, index, ok, gaps);
                continue;
            }
            let Some(w) = waiting.pop_front() else {
                fail(own, ok, gap("element-only-in-merge"), gaps);
                continue;
            };
            matched[w] = true;
            if !last.is_none_or(|last| w > last) {
                fail(own, ok, gap("order-differs-at-width"), gaps);
            }
            last = Some(w);
            if width[w].tag != tag
                && !(LAYOUT_ELEMENTS.contains(&tag)
                    && LAYOUT_ELEMENTS.contains(&width[w].tag.as_str()))
            {
                let mut gap = gap("element-differs-at-width");
                gap["widthTag"] = json!(width[w].tag);
                fail(own, ok, gap, gaps);
                continue;
            }
            self.compare(own, &width[w], tag, false, node, index, ok, gaps);
            if let Some(content) = &element.tree.content
                && content_at(content, slot) != text.as_deref().unwrap_or_default()
            {
                text_differs.insert(element.tree as *const Tree);
                gaps.push(gap("text-differs-at-width"));
            }
        }
        for (index, element) in width.iter().enumerate() {
            if !matched[index] {
                gaps.push(json!({"reason":"element-only-at-width","slot":slot,
                    "widthNodeId":element.node_id,"widthTag":element.tag}));
            }
        }
    }

    /// One element's values at this slot against the width's element for
    /// the same node. A wrapper carries only where its component sits.
    #[allow(clippy::too_many_arguments)]
    fn compare(
        &self,
        own: &[usize],
        theirs: &OwnedElement,
        tag: &str,
        placement_only: bool,
        node: &str,
        element: usize,
        ok: &mut [bool],
        gaps: &mut Vec<Value>,
    ) {
        let slot = self.slot;
        let their_tag = theirs.tag.as_str();
        // A reference's wrapper carries where an absolute instance sits, and
        // nothing of how the component lays itself out.
        let absolute =
            self.snapshot.nodes.get(node).is_some_and(|node| {
                node.typed_view().string("layoutPositioning") == Some("ABSOLUTE")
            });
        let placement = |name: &str, value: &str| {
            absolute && (POSITION_PROPS.contains(&name) || (name == "w" && value == "100%"))
        };
        let theirs = theirs
            .attributes
            .iter()
            .map(|(attribute, verified)| (attribute.name.as_str(), (written(attribute), *verified)))
            .filter(|(name, (value, _))| !placement_only || placement(name, value.as_str()))
            .collect::<BTreeMap<_, _>>();
        let mut names = BTreeSet::new();
        for index in own {
            let attribute = &self.attributes[*index];
            let name = attribute.name.as_str();
            names.insert(name);
            if placement_only && name == "display" {
                continue;
            }
            let here = slot_values(&attribute.source[name.len()..]).map(|values| {
                let value = effective(&values, slot);
                let unset = value == Some("initial")
                    && INHERITED.contains(&name)
                    && !self.inherited(element, name);
                settled(name, if unset { None } else { value }, tag)
            });
            let (there, verified) = theirs.get(name).map_or((None, true), |(value, verified)| {
                (Some(value.as_str()), *verified)
            });
            let there = settled(name, there, their_tag);
            if here.as_ref() != Some(&there) || !verified {
                ok[*index] = false;
                gaps.push(json!({"reason":if verified {"value-differs-at-width"} else {"unverified-at-width"},
                    "slot":slot,"widthNodeId":node,"element":element,"tag":tag,"property":name,
                    "merged":here,"width":there}));
            }
        }
        for (name, (value, _)) in &theirs {
            if !names.contains(name)
                && settled(name, Some(value.as_str()), their_tag) != settled(name, None, tag)
            {
                gaps.push(
                    json!({"reason":"attribute-only-at-width","slot":slot,"widthNodeId":node,
                    "element":element,"tag":tag,"property":name,"width":value}),
                );
            }
        }
        // Two layout elements differ only in what each does unwritten.
        if !placement_only && their_tag != tag {
            for name in implied_layout(tag)
                .iter()
                .chain(implied_layout(their_tag))
                .map(|(name, _)| *name)
                .chain(["display"])
            {
                if !names.contains(name)
                    && !theirs.contains_key(name)
                    && settled(name, None, tag) != settled(name, None, their_tag)
                {
                    gaps.push(json!({"reason":"element-differs-at-width","slot":slot,
                        "widthNodeId":node,"element":element,"tag":tag,"widthTag":their_tag,
                        "property":name}));
                }
            }
        }
    }

    /// Whether an element this one sits in sets `name` at this slot, so that
    /// leaving it unset inherits that rather than the initial value.
    fn inherited(&self, element: usize, name: &str) -> bool {
        let mut current = self.placed[element].parent;
        while let Some(index) = current {
            if self.by_element[index].iter().any(|a| {
                let attribute = &self.attributes[*a];
                attribute.name == name
                    && slot_values(&attribute.source[name.len()..])
                        .as_deref()
                        .and_then(|values| effective(values, self.slot))
                        .is_some_and(|value| value != "initial")
            }) {
                return true;
            }
            current = self.placed[index].parent;
        }
        false
    }

    /// A component reference: the component and the variant this width's
    /// instance picked. Everything the instance draws is the component's,
    /// so the width's elements for it are accounted for here; they are
    /// returned in order.
    #[allow(clippy::too_many_arguments)]
    fn reference(
        &self,
        element: &Placed<'_>,
        node: &str,
        own: &[usize],
        width: &[OwnedElement],
        matched: &mut [bool],
        ok: &mut [bool],
        gaps: &mut Vec<Value>,
    ) -> Vec<usize> {
        let slot = self.slot;
        let Some(instance) = self.snapshot.nodes.get(node) else {
            for index in own {
                ok[*index] = false;
            }
            gaps.push(json!({"reason":"no-source-at-slot","slot":slot,"widthNodeId":node}));
            return Vec::new();
        };
        let view = instance.typed_view();
        let drawn = width
            .iter()
            .enumerate()
            .filter(|(_, element)| {
                element
                    .node_id
                    .as_deref()
                    .is_some_and(|owner| descends(self.snapshot, owner, node))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for index in &drawn {
            matched[*index] = true;
        }
        let component = referenced_component_name(self.snapshot, view.name());
        if drawn.is_empty() || component != element.tree.component {
            for index in own {
                ok[*index] = false;
            }
            gaps.push(json!({"reason":if drawn.is_empty() {"element-only-in-merge"} else {"component-differs-at-width"},
                "slot":slot,"widthNodeId":node,"tag":element.tree.component,"widthComponent":component}));
            return drawn;
        }
        let expected = instance_variant_props(&view);
        let mut names = BTreeSet::new();
        for index in own {
            let attribute = &self.attributes[*index];
            names.insert(attribute.name.as_str());
            let here = slot_values(&attribute.source[attribute.name.len()..])
                .and_then(|values| effective(&values, slot).map(str::to_owned));
            if here.as_deref() != expected.get(&attribute.name).map(String::as_str) {
                ok[*index] = false;
                gaps.push(
                    json!({"reason":"variant-differs-at-width","slot":slot,"widthNodeId":node,
                    "property":attribute.name,"merged":here,"width":expected.get(&attribute.name)}),
                );
            }
        }
        for (name, value) in &expected {
            if !names.contains(name.as_str()) {
                gaps.push(
                    json!({"reason":"variant-only-at-width","slot":slot,"widthNodeId":node,
                    "property":name,"width":value}),
                );
            }
        }
        drawn
    }
}
