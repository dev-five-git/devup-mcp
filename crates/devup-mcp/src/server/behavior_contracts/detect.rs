use super::*;

pub(super) fn statistic(
    node: &RawNode,
    snapshot: &Snapshot,
    parents: &BTreeMap<&str, &RawNode>,
) -> Option<Value> {
    if node.node_type != "TEXT" {
        return None;
    }
    let text = string(node, "characters").trim();
    let prefix = text
        .chars()
        .next()
        .filter(|c| matches!(c, '$' | '₩' | '€' | '£' | '¥'))
        .map_or("", |c| &text[..c.len_utf8()]);
    let numeric = &text[prefix.len()..];
    let split = numeric
        .char_indices()
        .find(|(index, c)| {
            !(c.is_ascii_digit() || matches!(c, '.' | ',') || *index == 0 && *c == '-')
        })
        .map(|(index, _)| index)
        .unwrap_or(numeric.len());
    let (number, unit) = numeric.split_at(split);
    if number.is_empty()
        || number.ends_with('.')
        || number
            .split_once('.')
            .is_some_and(|(_, fraction)| fraction.contains(','))
        || number.trim_start_matches('-').starts_with('0')
            && number.trim_start_matches('-').len() > 1
            && !number.trim_start_matches('-').starts_with("0.")
    {
        return None;
    }
    let integer = number
        .trim_start_matches('-')
        .split('.')
        .next()
        .unwrap_or_default();
    if integer.contains(',') {
        let groups = integer.split(',').collect::<Vec<_>>();
        if !(1..=3).contains(&groups[0].len())
            || groups.iter().skip(1).any(|group| group.len() != 3)
        {
            return None;
        }
    }
    let target = number.replace(',', "").parse::<f64>().ok()?;
    if !target.is_finite()
        || unit.chars().any(|c| c.is_ascii_digit())
        || unit.chars().any(|c| {
            !(c.is_alphanumeric() || c.is_whitespace() || matches!(c, '%' | '/' | '°' | '$' | '₩'))
        })
    {
        return None;
    }
    let parent = parents.get(node.id.as_str()).copied().unwrap_or(node);
    let context = parents
        .get(parent.id.as_str())
        .copied()
        .filter(|ancestor| {
            !snapshot.roots.contains(&ancestor.id)
                || has(
                    string(ancestor, "name"),
                    &["stat", "metric", "summary", "counter", "통계"],
                )
        })
        .unwrap_or(parent);
    let label = nearby_text(context, snapshot, 2);
    if (1900.0..=2100.0).contains(&target)
        && unit.trim().starts_with('년')
        && !has(&label, &["experience", "career", "경력"])
    {
        return None;
    }
    let identity = format!(
        "{} {} {}",
        node_label(node),
        string(parent, "name"),
        string(context, "name")
    );
    let lower_identity = identity.to_lowercase();
    let calendar_name = lower_identity
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| matches!(word, "date" | "year"))
        && !has(
            &identity,
            &["experience", "research", "career", "경력", "연구"],
        );
    if has(
        &identity,
        &[
            "pagination",
            "page number",
            "page-number",
            "identifier",
            "patent number",
            "patent-number",
            "registration number",
            "application number",
            "record id",
            "등록번호",
            "출원번호",
            "페이지 번호",
            "연도",
            "날짜",
        ],
    ) || calendar_name
        || has(string(node, "name"), &["step", "순서", "index", "번호"])
    {
        return None;
    }
    let statistic_context = has(
        &label,
        &[
            "stat",
            "metric",
            "count",
            "통계",
            "경력",
            "보유",
            "개발 중",
            "시장",
            "비율",
            "성장률",
            "총계",
            "summary",
            "patent",
            "pipeline",
        ],
    );
    let unit_context = has(&label, &["%", "건", "개", "년", "억", "명", "달러"]);
    let emphasized = node
        .typed_view()
        .number("fontSize")
        .is_some_and(|size| size >= 28.0);
    if !statistic_context && !(emphasized && unit_context) {
        return None;
    }
    let precision = number
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    Some(candidate(
        "count-up",
        node,
        json!({"text":text,"context":label,"target":target,"precision":precision,"unit":unit,"prefix":prefix,"numericLabelRequiresReview":has(string(node,"name"), &["label","caption","라벨"])}),
        json!({
            "pattern":"shared-count-up","start":"first-viewport-entry","repeat":"once","valueSource":"validated API/DB value when business-owned","preserve":["decimal precision","unit","final value's responsive width","accessible final value"],
            "serverMarkup":"Render the final value on the server; use a client span/ref controller only for its animation.",
            "reducedMotion":"show final value","exclude":["dates","identifiers","step indices","pagination"],
            "labelPolicy":"Keep total/registered/applied and other captions fixed; verify which slot is a numeric value before binding.",
            "verify":["final integer and decimal values","live changed/empty values","entry/re-entry","resize from hidden responsive branches","no adjacent-label displacement"]
        }),
    ))
}

pub(super) fn marquee(
    node: &RawNode,
    _snapshot: &Snapshot,
    parents: &BTreeMap<&str, &RawNode>,
    result: &Value,
) -> Option<Value> {
    let captured = result["interactiveCandidates"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|candidate| candidate["nodeId"] == node.id && candidate["role"] == "marquee");
    let view = node.typed_view();
    let text = string(node, "characters");
    let single_line = node.node_type == "TEXT"
        && !text.contains(['\n', '\r'])
        && (view.number("maxLines") == Some(1.0)
            || view.string("textAutoResize") == Some("WIDTH_AND_HEIGHT"));
    let mut current = node;
    let mut seen = BTreeSet::new();
    let mut clipping = None;
    while seen.insert(current.id.as_str()) {
        let Some(parent) = parents.get(current.id.as_str()).copied() else {
            break;
        };
        if parent.typed_view().bool("clipsContent") == Some(true)
            && matches!((view.number("width"),parent.typed_view().number("width")),(Some(width),Some(clip)) if width>clip+1.0)
        {
            clipping = Some(parent.id.clone());
            break;
        }
        current = parent;
    }
    if captured.is_none() && !(single_line && clipping.is_some()) {
        return None;
    }
    Some(candidate(
        "marquee",
        node,
        json!({"singleLineOverflow":single_line&&clipping.is_some(),"clippingAncestorId":clipping,"interactionEvidence":captured}),
        json!({
            "pattern":"seamless-left-marquee","playback":"continuous linear loop when requested or accepted","duplicateContent":"one repeat group with aria-hidden on duplicates","seam":"measure one group's width; no gap or jump at wrap","pauseControls":"only when required by explicit policy or user preference; do not invent mandatory controls","reducedMotion":"honor the user's motion preference unless an explicit requirement overrides it","verify":["leftward movement","wrap continuity","short/long viewport","no-JS readable content","duplicate content excluded from accessible name"]
        }),
    ))
}

pub(super) fn progress(node: &RawNode, snapshot: &Snapshot) -> Option<Value> {
    if !matches!(
        node.node_type.as_str(),
        "FRAME" | "GROUP" | "INSTANCE" | "COMPONENT"
    ) {
        return None;
    }
    let children = node
        .typed_view()
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .collect::<Vec<_>>();
    let steps = children
        .iter()
        .filter(|child| has(string(child, "name"), &["step", "단계"]))
        .collect::<Vec<_>>();
    if steps.len() < 2 {
        return None;
    }
    let (descendants, _) = scoped_nodes(snapshot, std::slice::from_ref(&node.id));
    let progress=descendants.iter().filter(|child|{
        if has(string(child,"name"), &["progress", "진행선", "진행률"]) {return true}
        let view=child.typed_view();
        if !matches!((view.number("width"),view.number("height")),(Some(width),Some(height)) if width>=40.0 && height>0.0 && height<=6.0) {return false}
        view.child_ids().filter_map(|id|snapshot.nodes.get(id)).any(|fill| {
            let filled=fill.typed_view();
            matches!((view.number("width"),filled.number("width"),view.number("height"),filled.number("height")),(Some(track),Some(width),Some(height),Some(fill_height)) if width>=1.0 && width<track*0.95 && (height-fill_height).abs()<=1.0)
        })
    }).map(|child|child.id.clone()).collect::<Vec<_>>();
    if progress.is_empty() {
        return None;
    }
    let variants = steps
        .iter()
        .filter_map(|step| {
            step.typed_view()
                .value("variantProperties")
                .map(|properties| json!({"nodeId":step.id,"properties":properties}))
        })
        .collect::<Vec<_>>();
    Some(candidate(
        "progress-step-cycle",
        node,
        json!({"stepNodeIds":steps.iter().map(|child|&child.id).collect::<Vec<_>>(),"progressNodeIds":progress,"capturedStepVariants":variants}),
        json!({
            "pattern":"shared-progress-step-controller","cycle":"on progress completion advance to the next captured step and wrap using the actual step count","stateSource":"one active index owns highlight, description, image, progress and ARIA","content":"collect every captured active variant and preserve its image/text overrides","manualSelection":"pointer/keyboard selection restarts that step consistently","controls":"follow user playback requirements; do not invent pause/resume controls","transitions":"fade/height transitions without changing generated layout","timing":"choose and disclose an implementation duration unless captured motion defines it","verify":["progress fills before advance","last-to-first wrap","manual selection","different data lengths","cleanup and remount","reduced motion policy"]
        }),
    ))
}

pub(super) fn tabs(node: &RawNode, snapshot: &Snapshot, result: &Value) -> Option<Value> {
    let child_ids = node.typed_view().child_ids().collect::<BTreeSet<_>>();
    let captured = result["interactiveCandidates"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|candidate| {
            candidate["role"] == "tab"
                && candidate["nodeId"]
                    .as_str()
                    .is_some_and(|id| child_ids.contains(id))
        })
        .count();
    let named = has(
        string(node, "name"),
        &["tabs", "tabmenu", "tab menu", "탭 메뉴", "탭메뉴"],
    );
    if captured < 2 && !(named && child_ids.len() >= 2) {
        return None;
    }
    Some(candidate(
        "sliding-tab-indicator",
        node,
        json!({"capturedTabChildren":captured,"name":string(node,"name"),"childCount":child_ids.len(),"text":nearby_text(node,snapshot,1)}),
        json!({
            "pattern":"persistent-route-tabs","ownership":"nearest shared server layout (route group when only some siblings share the menu)","clientBoundary":"small pathname/query-aware tab indicator only","dom":"one set of links and one moving indicator; preserve their identity across route navigation","selection":"React owns aria-current; animation cleanup never restores a stale selection","routing":"real links and public URLs; never replace App Router with local screen state","motion":"measure active link bounds and transition indicator position/width","verify":["same DOM node survives navigation","actual intermediate animation frame","both directions","keyboard","history","direct refresh","resize"]
        }),
    ))
}

pub(super) fn view_switch(node: &RawNode, snapshot: &Snapshot) -> Option<Value> {
    let children = node.typed_view().child_ids().count();
    if !(2..=6).contains(&children) {
        return None;
    }
    let text = nearby_text(node, snapshot, 2);
    let pair = has(&text, &["썸네일", "thumbnail", "grid"]) && has(&text, &["목록", "list"]);
    if !pair {
        return None;
    }
    Some(candidate(
        "view-mode-state",
        node,
        json!({"text":text}),
        json!({
            "pattern":"single-source-view-toggle","stateSource":"same normalized URL query or application state that renders content","synchronize":["content","background","text","icon","opacity","aria-current or aria-pressed"],"responsive":"one responsive control; never keep fixed selected mobile/desktop copies","navigation":"preserve filters/sort and define page-reset policy","verify":["both modes and directions","exactly one selection","computed background/text/icon colours","keyboard focus and Enter","reload and browser history","resize"]
        }),
    ))
}

pub(super) fn editable_content(node: &RawNode) -> bool {
    node.node_type == "TEXT"
        && !string(node, "characters").trim().is_empty()
        && has(
            string(node, "name"),
            &[
                "product title",
                "product description",
                "product name",
                "제품명",
                "제품 설명",
                "development stage",
                "개발 단계",
            ],
        )
}

pub(super) fn palette(result: &Value, snapshot: &Snapshot, candidates: &mut Vec<Value>) {
    for record in result["instanceOverrides"].as_array().into_iter().flatten() {
        let fields = record["overriddenFields"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        if !fields
            .iter()
            .any(|field| has(field, &["fill", "stroke", "color"]))
        {
            continue;
        }
        let Some(node) = record["instanceNodeId"]
            .as_str()
            .and_then(|id| snapshot.nodes.get(id))
        else {
            continue;
        };
        candidates.push(candidate("palette-state-review",node,json!({"overrideFields":fields,"mainComponentId":record["mainComponentId"]}),json!({
            "review":["default","hover","active","focus-visible","disabled","border","text","icon"],"source":"captured variant states and actual project theme tokens","rule":"a colour override must not inherit the master's unrelated colour family","derivation":"do not invent a matching shade suffix; record any chosen fallback policy separately from Figma evidence","verify":["default/hover/press/keyboard focus computed colours","disabled/pending","instance colour overrides"]
        })));
    }
}

pub(super) fn page_contracts(
    scope: &[String],
    snapshot: &Snapshot,
    nodes: &[&RawNode],
    _candidates: &mut Vec<Value>,
) -> Vec<Value> {
    let mut contracts = Vec::new();
    for root in scope
        .iter()
        .filter_map(|id| snapshot.nodes.get(id))
        .filter(|node| matches!(node.node_type.as_str(), "FRAME" | "SECTION"))
    {
        let all = snapshot.nodes.values().collect::<Vec<_>>();
        let ancestry = parents(&all);
        let footer = nodes.iter().find(|node| {
            if !has(string(node, "name"), &["footer", "푸터", "하단 정보"]) {
                return false;
            }
            let mut current = Some(**node);
            let mut visited = BTreeSet::new();
            while let Some(node) = current {
                if node.id == root.id {
                    return true;
                }
                if !visited.insert(node.id.as_str()) {
                    break;
                }
                current = ancestry.get(node.id.as_str()).copied();
            }
            false
        });
        if let Some(footer) = footer {
            contracts.push(json!({"kind":"viewport-page-shell","rootNodeId":root.id,"footerNodeId":footer.id,"implemented":false,"verified":false,"implementation":{"minHeight":"100dvh","layout":"column flex with growing content / footer margin-top:auto","footer":"normal document flow; never fixed or based on today's measured header/footer height"},"verification":["short and empty content","long content without overlap","mobile/desktop including tall viewport","actual CSS viewport and zoom"]}));
        }
    }
    contracts
}

pub(super) fn section_entrance(
    node: &RawNode,
    snapshot: &Snapshot,
    scope: &[String],
    parents: &BTreeMap<&str, &RawNode>,
) -> Option<Value> {
    let eligible = |node: &RawNode| {
        matches!(node.node_type.as_str(), "FRAME" | "SECTION" | "INSTANCE")
            && !has(
                string(node, "name"),
                &[
                    "header",
                    "footer",
                    "gnb",
                    "navigation",
                    "breadcrumb",
                    "form",
                    "헤더",
                    "푸터",
                    "메뉴",
                    "폼",
                ],
            )
    };
    if !eligible(node)
        || node
            .typed_view()
            .number("height")
            .is_none_or(|height| height < 180.0)
    {
        return None;
    }
    let parent = parents.get(node.id.as_str())?;
    if !scope.contains(&parent.id) {
        return None;
    }
    let first = parent
        .typed_view()
        .child_ids()
        .filter_map(|id| snapshot.nodes.get(id))
        .find(|node| eligible(node))?;
    if first.id == node.id {
        return None;
    }
    Some(candidate(
        "section-entrance",
        node,
        json!({"sectionHeight":node.typed_view().number("height"),"belowFirstContentSection":true}),
        json!({
            "pattern":"devup-ui-static-keyframes-reveal","scope":"selected major below-fold sections, not every text or control","implementation":"Reveal.tsx: Devup UI keyframes() with animationFillMode both, animationTimeline view() and animationRange entry 0% entry min(100%, 240px) on an inner wrapper; no animation library","serverMarkup":"the resting style is the visible content and the hidden start exists only in the keyframes; never prehide SSR content","fallback":"the shared controller marks data-reveal-state once for groups on a clock (data-reveal=timed) and where view timelines are unsupported; it never hides content already on screen","motion":"short upward translation and fade, complete once the block is in view","reducedMotion":"_motionReduce animation none","focus":"_focusWithin animation none","timing":"implementation choice, not captured Figma motion","rules":"devfive-frontend/references/animation.md","verify":["start/middle/end frames","trigger position","scroll re-entry","fast scroll","reload","back/forward","bfcache","client navigation","delayed and failed images","focus","reduced motion","JavaScript disabled","stagger and progress timing"]
        }),
    ))
}
