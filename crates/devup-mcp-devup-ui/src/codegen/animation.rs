//! A timed Smart Animate as CSS keyframes — the plugin's `getReactionProps`.
//!
//! A frame that, after a timeout, Smart-Animates to another frame, which after
//! its own timeout animates to the next, is a chain; a chain that comes back
//! to where it started is a loop. The plugin walks the chain and writes what
//! changes from one frame to the next — position, size, opacity, fill,
//! rotation — as keyframes at the moments the changes land. The changes are
//! looked for in the frame's children first, matched by name, and each child
//! that changes carries its own animation; only when no child changes does
//! the frame itself carry one.
//!
//! The frames of a chain are top-level siblings the target's subtree does not
//! hold. The snapshot script gathers them as extra roots, so they are looked
//! up here in the snapshot; a destination that is not there is reported by
//! the caller as it was before.

use std::collections::BTreeSet;

use devup_mcp_figma::{RawNode, Snapshot};
use serde_json::{Map, Value};

use super::{
    component::{Prop, PropValue},
    layout::format_number,
    style::paint_string,
};

/// One step of a chain: the frame the previous one becomes, and how.
struct Step<'a> {
    node: &'a RawNode,
    duration: f64,
    easing: Option<String>,
    delay: f64,
}

/// The reaction that starts a chain: a timed Smart Animate to another frame.
struct Start {
    destination: String,
    duration: f64,
    easing: Option<String>,
    timeout: f64,
}

fn timed_smart_animates(node: &RawNode) -> Vec<Start> {
    let mut found = Vec::new();
    let Some(reactions) = node
        .typed_view()
        .value("reactions")
        .and_then(Value::as_array)
    else {
        return found;
    };
    for reaction in reactions {
        let trigger = reaction.get("trigger");
        if trigger
            .and_then(|trigger| trigger.get("type"))
            .and_then(Value::as_str)
            != Some("AFTER_TIMEOUT")
        {
            continue;
        }
        let timeout = trigger
            .and_then(|trigger| trigger.get("timeout"))
            .and_then(Value::as_f64)
            .unwrap_or_default();
        let Some(actions) = reaction.get("actions").and_then(Value::as_array) else {
            continue;
        };
        for action in actions {
            if action.get("type").and_then(Value::as_str) != Some("NODE") {
                continue;
            }
            let Some(transition) = action.get("transition") else {
                continue;
            };
            if transition.get("type").and_then(Value::as_str) != Some("SMART_ANIMATE") {
                continue;
            }
            let Some(destination) = action.get("destinationId").and_then(Value::as_str) else {
                continue;
            };
            // `transition.duration || 0.3`: a zero is the default too.
            let duration = transition
                .get("duration")
                .and_then(Value::as_f64)
                .filter(|duration| *duration != 0.0)
                .unwrap_or(0.3);
            found.push(Start {
                destination: destination.to_owned(),
                duration,
                easing: transition
                    .get("easing")
                    .and_then(|easing| easing.get("type"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                timeout,
            });
        }
    }
    found
}

/// The plugin's `buildAnimationChain`: every frame reached from `current`
/// by timed Smart Animates, and whether the chain comes back to `start`.
fn build_chain<'a>(
    snapshot: &'a Snapshot,
    start_id: &str,
    current: &'a RawNode,
    duration: f64,
    easing: Option<String>,
    delay: f64,
    visited: &BTreeSet<String>,
) -> (Vec<Step<'a>>, bool) {
    let mut chain = Vec::new();
    if current.id == start_id {
        return (chain, true);
    }
    let mut visited = visited.clone();
    visited.insert(current.id.clone());
    chain.push(Step {
        node: current,
        duration,
        easing,
        delay,
    });
    let mut is_loop = false;
    for start in timed_smart_animates(current) {
        if start.destination == start_id {
            is_loop = true;
            break;
        }
        if visited.contains(&start.destination) {
            continue;
        }
        let Some(next) = snapshot.nodes.get(&start.destination) else {
            continue;
        };
        if matches!(next.node_type.as_str(), "DOCUMENT" | "PAGE") {
            continue;
        }
        let (rest, looped) = build_chain(
            snapshot,
            start_id,
            next,
            start.duration,
            start.easing,
            start.timeout,
            &visited,
        );
        chain.extend(rest);
        if looped {
            is_loop = true;
        }
    }
    (chain, is_loop)
}

/// A node's parent, by its recorded `parentId` or by the node that lists it.
fn parent_of<'a>(snapshot: &'a Snapshot, node: &RawNode) -> Option<&'a RawNode> {
    node.typed_view()
        .string("parentId")
        .and_then(|parent_id| snapshot.nodes.get(parent_id))
        .or_else(|| {
            snapshot.nodes.values().find(|candidate| {
                candidate
                    .typed_view()
                    .child_ids()
                    .any(|child| child == node.id)
            })
        })
}

/// The plugin's `isPageRoot`: a frame that sits directly on a page, in a
/// Section or in a component set. A root carries its parent's type; a node
/// whose parent is in the snapshot is read through it.
fn is_page_root(snapshot: &Snapshot, node: &RawNode) -> bool {
    if matches!(node.node_type.as_str(), "SECTION" | "PAGE") {
        return false;
    }
    let view = node.typed_view();
    let parent_type = parent_of(snapshot, node)
        .map(|parent| parent.node_type.clone())
        .or_else(|| view.string("parentType").map(str::to_owned));
    parent_type.is_some_and(|kind| matches!(kind.as_str(), "SECTION" | "PAGE" | "COMPONENT_SET"))
}

/// The plugin's `generateSingleNodeDifferences`: what `to` has that `from`
/// does not, as the CSS that would move `from` there. Rotation is kept as a
/// delta under `rotationDelta`, to be summed along the chain.
fn differences(
    snapshot: &Snapshot,
    from: &RawNode,
    to: &RawNode,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) -> Map<String, Value> {
    let (from_view, to_view) = (from.typed_view(), to.typed_view());
    let mut changes = Map::new();

    // A frame's own place is only compared where it is not a top-level frame's
    // direct child: for those the plugin skips position, as it does here.
    let parent_is_page_root =
        parent_of(snapshot, to).is_some_and(|parent| is_page_root(snapshot, parent));
    if !parent_is_page_root
        && let (Some(from_x), Some(from_y), Some(to_x), Some(to_y)) = (
            from_view.number("x"),
            from_view.number("y"),
            to_view.number("x"),
            to_view.number("y"),
        )
        && (from_x != to_x || from_y != to_y)
    {
        changes.insert(
            "transform".to_owned(),
            Value::from(format!(
                "translate({}px, {}px)",
                format_number(to_x - from_x),
                format_number(to_y - from_y)
            )),
        );
    }

    if let (Some(from_w), Some(from_h), Some(to_w), Some(to_h)) = (
        from_view.number("width"),
        from_view.number("height"),
        to_view.number("width"),
        to_view.number("height"),
    ) {
        if from_w != to_w {
            changes.insert(
                "w".to_owned(),
                Value::from(format!("{}px", format_number(to_w))),
            );
        }
        if from_h != to_h {
            changes.insert(
                "h".to_owned(),
                Value::from(format!("{}px", format_number(to_h))),
            );
        }
    }

    if let (Some(from_opacity), Some(to_opacity)) =
        (from_view.number("opacity"), to_view.number("opacity"))
        && from_opacity != to_opacity
    {
        changes.insert("opacity".to_owned(), Value::from(format_number(to_opacity)));
    }

    if let (Some(from_fill), Some(to_fill)) = (
        from_view
            .value("fills")
            .and_then(Value::as_array)
            .and_then(|fills| fills.first()),
        to_view
            .value("fills")
            .and_then(Value::as_array)
            .and_then(|fills| fills.first()),
    ) && from_fill.get("type").and_then(Value::as_str) == Some("SOLID")
        && to_fill.get("type").and_then(Value::as_str) == Some("SOLID")
        && !same_color(from_fill.get("color"), to_fill.get("color"))
        && let Some(color) = paint_string(to_fill, Some(variable_tokens))
    {
        changes.insert("bg".to_owned(), Value::from(color));
    }

    if let (Some(from_rotation), Some(to_rotation)) =
        (from_view.number("rotation"), to_view.number("rotation"))
        && from_rotation != to_rotation
    {
        let mut delta = to_rotation - from_rotation;
        if delta > 180.0 {
            delta -= 360.0;
        } else if delta < -180.0 {
            delta += 360.0;
        }
        // Figma turns clockwise-negative.
        changes.insert("rotationDelta".to_owned(), Value::from(-delta));
    }
    changes
}

fn same_color(left: Option<&Value>, right: Option<&Value>) -> bool {
    let channel = |color: Option<&Value>, name: &str| {
        color
            .and_then(|color| color.get(name))
            .and_then(Value::as_f64)
            .unwrap_or_default()
    };
    ["r", "g", "b"]
        .iter()
        .all(|name| (channel(left, name) - channel(right, name)).abs() < 0.01)
}

fn easing_function(easing: Option<&str>) -> &'static str {
    match easing {
        Some("EASE_IN") => "ease-in",
        Some("EASE_OUT") => "ease-out",
        Some("EASE_IN_AND_OUT") => "ease-in-out",
        _ => "linear",
    }
}

/// The plugin's `fmtDuration`: three decimals at most, no trailing zeros.
fn format_duration(seconds: f64) -> String {
    let rounded = (seconds * 1000.0).round() / 1000.0;
    let text = format!("{rounded:.3}");
    let text = text.trim_end_matches('0');
    text.trim_end_matches('.').to_owned()
}

/// Keyframes from the changes along a chain, as the plugin assembles them,
/// with its rules kept: a property is written at 0% only where it takes more
/// than one value along the chain; a step writes a property only where it
/// differs from what the last keyframe left in effect; a property that
/// appears once is written that once; and a loop ends at 100% back where it
/// began, a rotation completing its turn. `None` when nothing changes.
fn keyframe_props(
    changes: &[Map<String, Value>],
    starting: &Map<String, Value>,
    chain: &[Step<'_>],
    is_loop: bool,
    delay: f64,
) -> Option<Vec<Prop>> {
    let total: f64 = chain.iter().map(|step| step.duration).sum();
    let effective_total = if is_loop {
        total + chain[0].duration
    } else {
        total
    };

    let mut animated: Vec<String> = Vec::new();
    for step_changes in changes {
        for key in step_changes.keys() {
            let key = if key == "rotationDelta" {
                "transform"
            } else {
                key.as_str()
            };
            if !animated.iter().any(|known| known == key) {
                animated.push(key.to_owned());
            }
        }
    }
    let has_rotation = changes
        .iter()
        .any(|step| step.contains_key("rotationDelta"));
    let values_of = |key: &str| {
        let mut values = BTreeSet::new();
        let mut occurrences = 0;
        for step in changes {
            if let Some(value) = step.get(key) {
                values.insert(value.to_string());
                occurrences += 1;
            }
        }
        (values.len(), occurrences)
    };

    let mut initial = Map::new();
    if !changes.is_empty() && !animated.is_empty() {
        for key in &animated {
            let needs_initial = (key == "transform" && has_rotation) || values_of(key).0 > 1;
            if !needs_initial {
                continue;
            }
            if let Some(value) = starting.get(key) {
                initial.insert(key.clone(), value.clone());
            } else if key == "transform" && has_rotation {
                initial.insert(key.clone(), Value::from("rotate(0deg)"));
            }
        }
    }

    let mut keyframes = Map::new();
    keyframes.insert("0%".to_owned(), Value::Object(initial.clone()));

    let has_multiple = |key: &str| {
        if key == "transform" && has_rotation {
            return true;
        }
        let (distinct, occurrences) = values_of(key);
        distinct > 1 || occurrences == 1
    };

    let mut accumulated = 0.0;
    let mut previous = initial.clone();
    let mut cumulative_rotation = 0.0;
    let mut changed = false;
    for (index, step) in chain.iter().enumerate() {
        accumulated += step.duration;
        let percentage = (accumulated / effective_total * 100.0).round();
        let key = format!("{}%", format_number(percentage));
        let mut step_changes = changes[index].clone();
        if let Some(delta) = step_changes
            .remove("rotationDelta")
            .and_then(|delta| delta.as_f64())
        {
            cumulative_rotation += delta;
            let rotate = format!("rotate({}deg)", format_number(cumulative_rotation));
            let transform = match step_changes.get("transform").and_then(Value::as_str) {
                Some(existing) if !existing.is_empty() => format!("{existing} {rotate}"),
                _ => rotate,
            };
            step_changes.insert("transform".to_owned(), Value::from(transform));
        }
        let mut incremental = Map::new();
        for (name, value) in &step_changes {
            if has_multiple(name) && previous.get(name) != Some(value) {
                incremental.insert(name.clone(), value.clone());
            }
        }
        if !incremental.is_empty() {
            for (name, value) in &incremental {
                previous.insert(name.clone(), value.clone());
            }
            keyframes.insert(key, Value::Object(incremental));
            changed = true;
        }
    }

    if is_loop && changed {
        let mut last = initial.clone();
        if cumulative_rotation != 0.0 {
            let full =
                cumulative_rotation.signum() * (cumulative_rotation.abs() / 360.0).ceil() * 360.0;
            last.insert(
                "transform".to_owned(),
                Value::from(format!("rotate({}deg)", format_number(full))),
            );
        }
        keyframes.insert("100%".to_owned(), Value::Object(last));
    }

    if !changed || keyframes.len() < 2 {
        return None;
    }
    let easing = chain[0].easing.as_deref();
    let mut props: Vec<Prop> = vec![
        (
            "animationName".to_owned(),
            PropValue::String(format!(
                "keyframes({})",
                serde_json::to_string_pretty(&Value::Object(keyframes)).unwrap_or_default()
            )),
        ),
        (
            "animationDuration".to_owned(),
            PropValue::String(format!("{}s", format_duration(effective_total))),
        ),
        (
            "animationTimingFunction".to_owned(),
            PropValue::String(easing_function(easing).to_owned()),
        ),
        (
            "animationFillMode".to_owned(),
            PropValue::String("forwards".to_owned()),
        ),
    ];
    if delay >= 0.01 {
        props.push((
            "animationDelay".to_owned(),
            PropValue::String(format!("{}s", format_duration(delay))),
        ));
    }
    if is_loop {
        props.push((
            "animationIterationCount".to_owned(),
            PropValue::String("infinite".to_owned()),
        ));
    }
    Some(props)
}

fn child_named<'a>(snapshot: &'a Snapshot, node: &RawNode, name: &str) -> Option<&'a RawNode> {
    node.typed_view()
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .find(|child| child.typed_view().name() == Some(name))
}

/// The plugin's `generateChildAnimations`: for each child of the start
/// frame, by name, the keyframes of what changes in the same-named child
/// along the chain.
fn child_animations(
    snapshot: &Snapshot,
    start: &RawNode,
    chain: &[Step<'_>],
    is_loop: bool,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) -> Vec<(String, Vec<Prop>)> {
    let mut animations = Vec::new();
    let children = start
        .typed_view()
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .collect::<Vec<_>>();
    for child in &children {
        let Some(name) = child.typed_view().name() else {
            continue;
        };
        let mut changes = Vec::new();
        for (index, step) in chain.iter().enumerate() {
            let previous = if index == 0 {
                start
            } else {
                chain[index - 1].node
            };
            let pair = (
                child_named(snapshot, previous, name),
                child_named(snapshot, step.node, name),
            );
            changes.push(match pair {
                (Some(from), Some(to)) => differences(snapshot, from, to, variable_tokens),
                _ => Map::new(),
            });
        }
        let starting = child_named(snapshot, chain[0].node, name)
            .map(|first| differences(snapshot, first, child, variable_tokens))
            .unwrap_or_default();
        if let Some(props) = keyframe_props(&changes, &starting, chain, is_loop, chain[0].delay) {
            animations.push((name.to_owned(), props));
        }
    }
    animations
}

/// What one start frame's chain animates: its children by name, or, when
/// none of them changes, the frame itself.
enum Animated {
    Children(Vec<(String, Vec<Prop>)>),
    Own(Vec<Prop>),
    Nothing,
}

fn animated_by(
    snapshot: &Snapshot,
    node: &RawNode,
    variable_tokens: &std::collections::BTreeMap<String, String>,
) -> Animated {
    for start in timed_smart_animates(node) {
        let Some(destination) = snapshot.nodes.get(&start.destination) else {
            continue;
        };
        if matches!(destination.node_type.as_str(), "DOCUMENT" | "PAGE") {
            continue;
        }
        let (chain, is_loop) = build_chain(
            snapshot,
            &node.id,
            destination,
            start.duration,
            start.easing.clone(),
            start.timeout,
            &BTreeSet::new(),
        );
        if chain.is_empty() {
            continue;
        }
        let children = child_animations(snapshot, node, &chain, is_loop, variable_tokens);
        if !children.is_empty() {
            return Animated::Children(children);
        }
        let changes = chain
            .iter()
            .enumerate()
            .map(|(index, step)| {
                let previous = if index == 0 {
                    node
                } else {
                    chain[index - 1].node
                };
                differences(snapshot, previous, step.node, variable_tokens)
            })
            .collect::<Vec<_>>();
        let starting = differences(snapshot, chain[0].node, node, variable_tokens);
        if let Some(props) = keyframe_props(&changes, &starting, &chain, is_loop, start.timeout) {
            return Animated::Own(props);
        }
    }
    Animated::Nothing
}

/// The animation props a node carries, if any: those its parent's chain
/// gives it by name, else its own.
pub(super) fn push_animation_props(
    snapshot: &Snapshot,
    node: &RawNode,
    variable_tokens: &std::collections::BTreeMap<String, String>,
    props: &mut Vec<Prop>,
) {
    if let Some(parent) = parent_of(snapshot, node)
        && let Animated::Children(children) = animated_by(snapshot, parent, variable_tokens)
        && let Some(name) = node.typed_view().name()
        && let Some((_, animation)) = children.into_iter().find(|(child, _)| child == name)
    {
        props.extend(animation);
        return;
    }
    if let Animated::Own(animation) = animated_by(snapshot, node, variable_tokens) {
        props.extend(animation);
    }
}

/// Whether a node's timed Smart Animate points at a frame the snapshot does
/// not hold, so that nothing can be written for it.
pub(super) fn has_unreachable_destination(snapshot: &Snapshot, node: &RawNode) -> bool {
    timed_smart_animates(node)
        .iter()
        .any(|start| !snapshot.nodes.contains_key(&start.destination))
}

#[cfg(test)]
mod tests {
    use super::format_duration;

    #[test]
    fn durations_are_written_like_the_plugin_s() {
        assert_eq!(format_duration(1.6), "1.6");
        assert_eq!(format_duration(0.2), "0.2");
        assert_eq!(format_duration(1.0), "1");
        assert_eq!(format_duration(0.1234), "0.123");
        assert_eq!(format_duration(0.30000001), "0.3");
    }
}
