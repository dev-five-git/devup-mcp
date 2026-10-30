

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
    context: &mut MergeContext<'_>,
) -> Vec<Tree> {
    let maps = align_children(by_slot, context);
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
            merged.extend(merge_trees(&children, precedence, context));
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
    pub diagnostics: Vec<devup_mcp_figma::Diagnostic>,
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
    /// Each opening element of `tsx`, in document order: the node every
    /// drawn width made it from, and the widths it is drawn hidden at.
    pub elements: Vec<MergedElement>,
}

/// One merged element and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedElement {
    /// Inside another element's text (a `<br />`), not a node of its own.
    pub in_text: bool,
    /// (frame width, node id) for each width that draws it, widest first.
    pub ids: Vec<(u32, String)>,
    /// The drawn widths it is present but hidden at.
    pub hidden_at: Vec<u32>,
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
    merge_discovered_breakpoints(snapshot, options, &found)
}

pub fn merge_confirmed_breakpoints(
    snapshot: &Snapshot,
    options: &CodegenOptions,
) -> Result<Option<MergedScreen>, DevupError> {
    let widths = snapshot.roots.iter().filter_map(|id| {
        Some(("confirmed", snapshot.nodes.get(id)?.typed_view().number("width")?))
    }).collect::<Vec<_>>();
    if widths.len() != snapshot.roots.len() || !breakpoint_family_is_possible(&widths) {
        return Ok(None);
    }
    let found = snapshot.roots.iter().enumerate().map(|(rank, id)| Breakpoint {
        rank, node_id: id.clone(),
    }).collect::<Vec<_>>();
    merge_discovered_breakpoints(snapshot, options, &found)
}

fn merge_discovered_breakpoints(
    snapshot: &Snapshot,
    options: &CodegenOptions,
    found: &[Breakpoint],
) -> Result<Option<MergedScreen>, DevupError> {
    if found.len() < 2 {
        return Ok(None);
    }
    let mut by_slot: BySlot<Tree> = std::array::from_fn(|_| None);
    let mut roots: BySlot<String> = std::array::from_fn(|_| None);
    let mut precedence: Precedence = [usize::MAX; SLOTS];
    let mut slot_widths: BySlot<u32> = [None; SLOTS];
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
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a frame is a small positive number of pixels wide"
            )]
            {
                slot_widths[slot] = Some(width.max(0.0).round() as u32);
            }
        }
    }
    slots.sort_unstable();
    let mut context = MergeContext { snapshot, notes: Vec::new(), diagnostics: Vec::new(), slot_widths };
    let Some(mut tree) = merge_trees(&by_slot, &precedence, &mut context) else {
        return Ok(None);
    };
    // Each width's root is as wide as its frame, so the merged root would
    // say `w={['360px', ..., '1920px']}`: the frame widths, not a layout. A
    // page or section is a block that already takes its container's width.
    let root_fluid = tree
        .props
        .get("w")
        .and_then(|value| slot_values(value))
        .is_some_and(|values| {
            values.len() > 1
                && values
                    .iter()
                    .flatten()
                    .all(|value| value.ends_with("px"))
        });
    if root_fluid {
        tree.props.remove("w");
    }
    let mut components = BTreeSet::new();
    collect_components(&tree, &mut components);
    let tsx = render_merged(&tree, 2);
    let (contract, gaps) = verify_merge(snapshot, options, &roots, &tree, &tsx, root_fluid);
    let elements = merged_elements(&tree, &slot_widths);
    Ok(Some(MergedScreen {
        diagnostics: context.diagnostics,
        elements,
        verified: gaps.is_empty() && contract.iter().all(|entry| entry["verified"] == true),
        tsx,
        components,
        slots,
        unrepresented: context.notes,
        contract,
        gaps,
    }))
}

/// Where each element of the merged module came from, in the order
/// `render_merged` writes the opening elements.
fn merged_elements(tree: &Tree, slot_widths: &BySlot<u32>) -> Vec<MergedElement> {
    let mut placed = Vec::new();
    place(tree, None, [true; SLOTS], &mut placed);
    placed
        .iter()
        .map(|placed| {
            if placed.in_text {
                return MergedElement {
                    in_text: true,
                    ids: Vec::new(),
                    hidden_at: Vec::new(),
                };
            }
            let mut ids = Vec::new();
            let mut hidden_at = Vec::new();
            for (slot, width) in slot_widths.iter().enumerate() {
                let Some(width) = width else {
                    continue;
                };
                match placed.tree.slots.get(slot).cloned().flatten() {
                    Some((node, _)) if placed.shown[slot] => ids.push((*width, node)),
                    _ if !placed.shown[slot] => hidden_at.push(*width),
                    // Shown with no node of its own at that width: nothing
                    // in that frame to compare it with.
                    _ => {}
                }
            }
            ids.sort_by_key(|(width, _)| std::cmp::Reverse(*width));
            hidden_at.sort_by_key(|width| std::cmp::Reverse(*width));
            MergedElement {
                in_text: false,
                ids,
                hidden_at,
            }
        })
        .collect()
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
/// reference stands for the instance's whole subtree. Its name, variant and
/// placement are compared, but what it draws belongs to a file this module
/// does not contain, so a module that shows one is not verified. An element
/// no collected width draws is read at none of them, so its values are not
/// verified.
///
/// This reads the code; it does not render it. It is what makes the merged
/// attributes as verified as the per-width ones, and no more.
fn verify_merge(
    snapshot: &Snapshot,
    options: &CodegenOptions,
    roots: &BySlot<String>,
    tree: &Tree,
    tsx: &str,
    root_fluid: bool,
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
                        root_fluid,
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
    root_fluid: bool,
}
