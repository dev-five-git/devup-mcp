

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
                && !(self.root_fluid && element == 0 && *name == "w")
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
        // The reference draws whatever its component's own file draws, which
        // this module does not contain, and an instance can override its
        // component's text, size or paint where the reference cannot say so.
        // Matching the name and variant is not the same drawing.
        gaps.push(
            json!({"reason":"component-body-unverified","slot":slot,"widthNodeId":node,
            "component":component,"drawnElements":drawn.len()}),
        );
        drawn
    }
}
