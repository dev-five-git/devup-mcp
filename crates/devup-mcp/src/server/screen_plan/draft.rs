

/// The first exclude keyword in any of `names`, unless an include keyword
/// forces them in.
fn excluded_by<'a>(names: &[(&'a str, &'a str)], config: &PlanConfig) -> Option<String> {
    let include = config.include_keywords.as_deref().unwrap_or_default();
    if names
        .iter()
        .any(|(_, name)| include.iter().any(|keyword| keyword_in(name, keyword)))
    {
        return None;
    }
    let defaults = EXCLUDE_WORDS
        .iter()
        .map(|word| (*word).to_owned())
        .collect::<Vec<_>>();
    let keywords = config.exclude_keywords.as_ref().unwrap_or(&defaults);
    names.iter().find_map(|(place, name)| {
        keywords
            .iter()
            .find(|keyword| keyword_in(name, keyword))
            .map(|keyword| format!("{place} \"{name}\" contains \"{keyword}\""))
    })
}

/// Why a frame is not for implementation, or `None`. The names of the
/// linked node and its ancestors are left out: the user pointed there, which
/// is asked once rather than excluded (see the `scope` question).
fn exclusion_reason(
    screen: &PlanScreenInput,
    linked: &[String],
    config: &PlanConfig,
) -> Option<String> {
    if screen.visible == Some(false) {
        return Some("hidden in Figma".into());
    }
    if screen.opacity.is_some_and(|opacity| opacity <= 0.0) {
        return Some("zero opacity in Figma".into());
    }
    let mut names = vec![("frame", screen.name.as_str())];
    if let Some(section) = screen.section_name.as_deref() {
        names.push(("section", section));
    }
    for ancestor in screen
        .breadcrumb
        .split_last()
        .map(|(_, ancestors)| ancestors)
        .unwrap_or_default()
    {
        names.push(("parent", ancestor.as_str()));
    }
    names.retain(|(place, name)| *place == "frame" || !linked.iter().any(|link| link == name));
    let mut seen = BTreeSet::new();
    names.retain(|(_, name)| seen.insert(*name));
    excluded_by(&names, config)
}

/// One devup_figma_explore candidate node as a screen to plan. Its parent
/// counts as its section only when the breadcrumb shows one between the page
/// and the frame; a frame drawn straight on the page is in no section.
pub(super) fn screen_from_explore(node: &Value) -> Option<PlanScreenInput> {
    let breadcrumb = array(&node["breadcrumb"])
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let parent = node["parentId"].as_str().map(str::to_owned);
    let in_section = breadcrumb.len() >= 3;
    Some(PlanScreenInput {
        node_id: node["nodeId"].as_str()?.to_owned(),
        name: text(&node["name"]).to_owned(),
        width: node["bounds"]["width"].as_f64()?,
        height: node["bounds"]["height"].as_f64(),
        section: parent.clone().filter(|_| in_section),
        section_name: in_section.then(|| breadcrumb[breadcrumb.len() - 2].clone()),
        ancestors: parent.into_iter().collect(),
        visible: node["visible"].as_bool(),
        opacity: node["opacity"].as_f64(),
        breadcrumb,
        ..PlanScreenInput::default()
    })
}

/// Whether a frame is under the node the user linked. A frame that carries
/// no ancestry at all came from the caller's own list and is taken as given;
/// one whose ancestry is known and does not reach the link is never a
/// candidate.
pub(super) fn in_scope(
    screen: &PlanScreenInput,
    anchor_id: Option<&str>,
    anchor_name: Option<&str>,
) -> bool {
    let Some(anchor) = anchor_id else {
        return true;
    };
    if screen.node_id == anchor
        || screen.section.as_deref() == Some(anchor)
        || screen.ancestors.iter().any(|id| id == anchor)
    {
        return true;
    }
    let ancestor_names = screen
        .breadcrumb
        .split_last()
        .map(|(_, ancestors)| ancestors)
        .unwrap_or_default();
    if anchor_name.is_some_and(|name| ancestor_names.iter().any(|ancestor| ancestor == name)) {
        return true;
    }
    screen.section.is_none() && screen.ancestors.is_empty() && screen.breadcrumb.is_empty()
}

fn with_when(mut question: Value, conditions: &[Value]) -> Value {
    if !conditions.is_empty() {
        let mut all = array(&question["when"]).to_vec();
        all.extend(conditions.iter().cloned());
        question["when"] = json!(all);
    }
    question
}

const SECTIONS_SEPARATE: &str = "Separate pages: the sections are different pages";
const SECTIONS_SINGLE: &str = "One responsive page: the sections only split by device";
const KEEP_EXCLUDED: &str = "Keep it excluded";
const BUILD_IT: &str = "Build it";
const ACCEPT_EXCLUSIONS: &str = "Accept the exclusions";
const PLAN_ANYWAY: &str = "Plan them: this is what I linked";
const NOTHING_HERE: &str = "Nothing here is for implementation";

/// One route the evidence names, what kind of evidence it is, and where.
#[derive(Debug, Clone)]
struct RouteHit {
    route: String,
    kind: &'static str,
    detail: String,
}

/// `name-match` is the only kind that is a guess: a layer name compared with
/// a route segment. Every other kind is something the project or the caller
/// already says.
const NAME_MATCH: &str = "name-match";

/// Every route the evidence supports for these frames.
fn route_hits(
    frames: &[Frame],
    words: &[String],
    targets: &BTreeMap<String, String>,
    evidence: &ProjectEvidence,
    notes: &mut Vec<String>,
) -> Vec<RouteHit> {
    let mut hits = Vec::new();
    for frame in frames {
        if let Some(target) = targets.get(&frame.node_id) {
            match normalize_route(target) {
                Some(route) => hits.push(RouteHit {
                    route,
                    kind: "navigationTargets",
                    detail: format!("navigationTargets[{}]", frame.node_id),
                }),
                None => notes.push(format!(
                    "navigationTargets[{}] = {target:?} is not an app route and was not used.",
                    frame.node_id
                )),
            }
        }
        if let Some((route, file)) = evidence.figma_nodes.get(&frame.node_id) {
            hits.push(RouteHit {
                route: route.clone(),
                kind: "figma-node",
                detail: format!("data-figma-node-id=\"{}\" in {file}", frame.node_id),
            });
        }
        for (alias, route) in &evidence.config.route_aliases {
            if alias == &frame.name || compact(alias) == compact(&words.join("")) {
                hits.push(RouteHit {
                    route: route.clone(),
                    kind: "config",
                    detail: format!("{CONFIG_FILE} routeAliases[{alias:?}]"),
                });
            }
        }
    }
    let name = compact(&words.join(""));
    for (label, href, location) in &evidence.nav_links {
        if compact(label).is_empty() || compact(label) != name {
            continue;
        }
        if let Some((route, _)) = evidence
            .routes
            .iter()
            .find(|(route, _)| super::navigation::matches_route(route, href))
        {
            hits.push(RouteHit {
                route: route.clone(),
                kind: "nav-label",
                detail: format!("nav-label: {label:?} -> {href} ({location})"),
            });
        }
    }
    if hits.is_empty() {
        let screen_slug = slug(words);
        let is_root = words.iter().any(|word| ROOT_WORDS.contains(&word.as_str()));
        for (route, page) in &evidence.routes {
            // The last segment only: `/news/[id]` is a detail page, and a
            // screen named News is the list, not that page.
            let last = route
                .trim_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or_default();
            if (!screen_slug.is_empty() && last == screen_slug) || (route == "/" && is_root) {
                hits.push(RouteHit {
                    route: route.clone(),
                    kind: NAME_MATCH,
                    detail: format!("layer name matches the existing route {route} ({page})"),
                });
            }
        }
    }
    hits
}

/// The route the hits agree on, if they agree.
fn agreed(hits: &[RouteHit]) -> Option<&RouteHit> {
    let distinct = hits
        .iter()
        .map(|hit| hit.route.as_str())
        .collect::<BTreeSet<_>>();
    (distinct.len() == 1).then(|| &hits[0])
}

fn existing_page<'a>(route: &str, evidence: &'a ProjectEvidence) -> Option<&'a str> {
    evidence
        .routes
        .iter()
        .find(|(candidate, _)| candidate == route)
        .map(|(_, page)| page.as_str())
}

fn planned_page(route: &str, app_dir: &str) -> String {
    let route = route.trim_matches('/');
    if route.is_empty() {
        format!("{app_dir}/page.tsx")
    } else {
        format!("{app_dir}/{route}/page.tsx")
    }
}

/// A condition on another question's answer: this question is asked only
/// when that one resolved to `action`. The caller sees the label.
fn when(question: &str, label: &str, action: Value) -> Value {
    json!({"question": question, "answer": label, "action": action})
}

const SINGLE: &str = "One responsive page";
const SEPARATE: &str = "Separate pages";
const ACCEPT_ALL: &str = "Accept all";
const REVIEW: &str = "Review individually";

/// Builds the draft plan. Pure: every input is passed in, so the rules are
/// tested without Figma or a project on disk.
#[cfg(test)]
pub(super) fn draft_plan(
    screens: &[PlanScreenInput],
    targets: &BTreeMap<String, String>,
    evidence: &ProjectEvidence,
    source: Value,
    notes: Option<&str>,
) -> Result<Value, DevupError> {
    draft_plan_with(screens, &[], &[], targets, evidence, source, notes)
}

/// [`draft_plan`] with Figma nodes that may become the favicon or share
/// image.
pub(super) fn draft_plan_with(
    screens: &[PlanScreenInput],
    assets: &[PlanAssetInput],
    extra: &[ExtraQuestionInput],
    targets: &BTreeMap<String, String>,
    evidence: &ProjectEvidence,
    source: Value,
    notes: Option<&str>,
) -> Result<Value, DevupError> { include!("draft_plan_with.rs") }

/// How deep a question sits behind the answers it depends on. A question is
/// asked in a round after every question it depends on.
fn depth(question: &Value, questions: &[Value]) -> usize {
    array(&question["when"])
        .iter()
        .filter_map(|condition| {
            questions
                .iter()
                .find(|other| other["id"] == condition["question"])
        })
        .map(|parent| depth(parent, questions) + 1)
        .max()
        .unwrap_or(0)
}

/// The questions split into rounds a question tool takes in one call each:
/// at most four questions of at most four options, every question exactly
/// once, and a conditional question always in a later round than the one it
/// depends on.
pub(super) fn question_rounds(questions: &[Value], per_round: usize) -> Vec<Value> {
    let mut by_depth = BTreeMap::<usize, Vec<&Value>>::new();
    for question in questions {
        by_depth
            .entry(depth(question, questions))
            .or_default()
            .push(question);
    }
    let mut rounds = Vec::new();
    for level in by_depth.values() {
        for chunk in level.chunks(per_round.max(1)) {
            let shaped = ask_shape_with(
                questions,
                &chunk
                    .iter()
                    .map(|question| (*question).clone())
                    .collect::<Vec<_>>(),
            );
            rounds.push(json!({"round": rounds.len() + 1, "questions": shaped}));
        }
    }
    rounds
}

fn plan_path(project_root: &str, relative: &str) -> String {
    Path::new(project_root)
        .join(relative)
        .to_string_lossy()
        .into_owned()
}

/// Writes the plan JSON and its Markdown twin in one transaction, so the two
/// never describe different plans.
fn write_plan(
    policy: &OutputPolicy,
    project_root: &str,
    plan: &Value,
) -> Result<Value, DevupError> {
    let json_target = policy.resolve(&plan_path(project_root, PLAN_FILE))?;
    let markdown_target = policy.resolve(&plan_path(project_root, PLAN_MARKDOWN))?;
    let written = json!([
        json_target.display_path().display().to_string(),
        markdown_target.display_path().display().to_string(),
    ]);
    let bytes = serde_json::to_vec_pretty(plan)
        .map_err(|error| invalid(format!("Cannot serialize the screen plan: {error}")))?;
    let mut transaction = OutputTransaction::new();
    transaction.stage("screenPlan", json_target, &bytes)?;
    transaction.stage(
        "screenPlanMarkdown",
        markdown_target,
        markdown(plan).as_bytes(),
    )?;
    transaction.commit()?;
    Ok(written)
}

/// Reads `<projectRoot>/.devup/screen-plan.json`, or `None` when there is
/// none. A file that exists but is not a plan is an error, never "no plan".
pub(super) fn read_plan(project_root: &str) -> Result<Option<Value>, DevupError> {
    let path = Path::new(project_root).join(PLAN_FILE);
    let Ok(metadata) = std::fs::metadata(&path) else {
        return Ok(None);
    };
    if metadata.len() > MAX_PLAN_BYTES as u64 {
        return Err(invalid(format!(
            "{} exceeds the plan size limit.",
            path.display()
        )));
    }
    let bytes = std::fs::read(&path)
        .map_err(|error| invalid(format!("Cannot read {}: {error}", path.display())))?;
    let plan: Value = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("{} is not valid JSON: {error}", path.display())))?;
    if plan["kind"] != PLAN_KIND || plan["schemaVersion"] != SCHEMA_VERSION {
        return Err(invalid(format!(
            "{} is not a devup screen plan; create one with devup_screen_plan action draft.",
            path.display()
        )));
    }
    Ok(Some(plan))
}

fn require_root(input: &ScreenPlanInput) -> Result<&str, DevupError> {
    input
        .project_root
        .as_deref()
        .filter(|root| !root.trim().is_empty())
        .ok_or_else(|| invalid(format!("action {} needs projectRoot.", input.action)))
}

/// `draft`: plan the given screens against the project and write both files.
#[cfg(test)]
pub(super) async fn draft(
    input: &ScreenPlanInput,
    screens: &[PlanScreenInput],
    source: Value,
    policy: &OutputPolicy,
) -> Result<Value, DevupError> {
    draft_with(input, screens, &input.assets, source, policy).await
}

/// [`draft`] with the Figma asset candidates an explore found beside the
/// screens.
pub(super) async fn draft_with(
    input: &ScreenPlanInput,
    screens: &[PlanScreenInput],
    assets: &[PlanAssetInput],
    source: Value,
    policy: &OutputPolicy,
) -> Result<Value, DevupError> {
    let root = require_root(input)?;
    let evidence = project_evidence(root).await?;
    let mut plan = draft_plan_with(
        screens,
        assets,
        &input.extra_questions,
        &input.navigation_targets,
        &evidence,
        source,
        input.notes.as_deref(),
    )?;
    let per_round = input
        .max_questions_per_round
        .unwrap_or(MAX_QUESTIONS_PER_ROUND);
    if !(1..=50).contains(&per_round) {
        return Err(invalid("maxQuestionsPerRound must be between 1 and 50."));
    }
    plan["maxQuestionsPerRound"] = json!(per_round);
    plan["createdAt"] = json!(now());
    let replaced = read_plan(root)
        .ok()
        .flatten()
        .filter(|previous| previous["status"] == "confirmed")
        .map(|previous| previous["planId"].clone());
    let written = write_plan(policy, root, &plan)?;
    let questions = ask_shape(array(&plan["questions"]));
    let rounds = question_rounds(array(&plan["questions"]), per_round);
    let unverified = array(&plan["screens"])
        .iter()
        .filter(|screen| screen["routeEvidence"] == "UNVERIFIED")
        .count();
    Ok(json!({
        "status": "draft",
        "planId": plan["planId"],
        "screens": screen_table(&plan),
        "questionRounds": rounds,
        "questions": questions,
        "questionCount": questions.len(),
        "askInstruction": ASK_INSTRUCTION,
        "maxQuestionsPerRound": per_round,
        "site": plan["site"],
        "defaults": plan["defaults"],
        "heuristics": plan["heuristics"],
        "excluded": plan["excluded"],
        "outOfScope": plan["outOfScope"],
        "config": plan["config"],
        "unverifiedRoutes": unverified,
        "inventoryNote": plan["inventoryNote"],
        "writtenTo": written,
        "replacedConfirmedPlan": replaced,
        "nextAction": if questions.is_empty() {
            json!({"tool": "devup_screen_plan", "arguments": {"action": "confirm", "projectRoot": root, "planId": plan["planId"], "answers": {}},
                "how": "Nothing is open. Show the user the screens table, then confirm with empty answers."})
        } else {
            json!({"tool": "devup_screen_plan", "arguments": {"action": "confirm", "projectRoot": root, "planId": plan["planId"], "answers": "<question id -> chosen label, for every question that applies>"},
                "how": ASK_INSTRUCTION})
        },
    }))
}

pub(super) fn strip_recommended(label: &str) -> &str {
    label
        .trim()
        .strip_suffix(RECOMMENDED)
        .unwrap_or(label.trim())
}

/// The action an answer selects, or the custom text when it matches no
/// option. A custom answer to a question that places a screen must itself be
/// a route; anything else is recorded and the recommended action applies.
fn resolve_answer(
    question: &Value,
    answer: &AnswerInput,
) -> Result<(Value, Option<String>), DevupError> {
    let id = text(&question["id"]);
    let values = answer.values();
    if question["multiple"] == true {
        let labels = values
            .iter()
            .map(|value| strip_recommended(value))
            .collect::<Vec<_>>();
        return Ok((json!({"extra": labels}), None));
    }
    let [value] = values.as_slice() else {
        return Err(invalid(format!("{id} takes exactly one answer.")));
    };
    let label = strip_recommended(value);
    if let Some(found) = array(&question["resolve"])
        .iter()
        .find(|option| text(&option["label"]) == label)
    {
        return Ok((found["action"].clone(), None));
    }
    let route_of = |value: &str| normalize_route(value).filter(|route| valid_route(route));
    match text(&question["kind"]) {
        "route" | "modal" => route_of(label)
            .map(|route| (json!({"route": route}), Some(label.to_owned())))
            .ok_or_else(|| {
                invalid(format!(
                    "{id}: a custom answer must be an app route such as /about, not {label:?}."
                ))
            }),
        // One route is one page at that URL; one route per frame, in frame
        // order, is separate pages at those URLs.
        "group" => {
            let routes = label
                .split([',', ';', '\n'])
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(|part| route_of(part.rsplit('=').next().unwrap_or(part).trim()))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    invalid(format!(
                        "{id}: a custom answer must be one route, or one route per frame such as \"/a, /b\"; not {label:?}."
                    ))
                })?;
            match routes.as_slice() {
                [route] => Ok((
                    json!({"group": "single", "route": route}),
                    Some(label.to_owned()),
                )),
                _ => Ok((
                    json!({"group": "separate", "routes": routes}),
                    Some(label.to_owned()),
                )),
            }
        }
        "link" => Ok((
            json!({"link": normalize_route(label)}),
            Some(label.to_owned()),
        )),
        "extra" => Ok((json!({"extra": label}), Some(label.to_owned()))),
        // A custom site answer is the file, name or URL itself.
        "site" => {
            let field = text(&question["field"]);
            match field {
                "favicon" | "ogImage" => Ok((
                    json!({field: {"source": "provide", "path": label}}),
                    Some(label.to_owned()),
                )),
                "siteName" | "appName" => Ok((
                    json!({field: {"source": "answer", "value": label}}),
                    Some(label.to_owned()),
                )),
                "app" => Ok((
                    json!({field: {"dir": label, "appDir": format!("{}/src/app", label.trim_end_matches('/'))}}),
                    Some(label.to_owned()),
                )),
                "siteUrl" if label.starts_with("https://") || label.starts_with("http://") => Ok((
                    json!({field: {"source": "answer", "value": label}}),
                    Some(label.to_owned()),
                )),
                "siteUrl" => Err(invalid(format!(
                    "{id}: a custom answer must be an http(s) URL, not {label:?}."
                ))),
                _ => Ok((
                    array(&question["resolve"])
                        .first()
                        .map(|option| option["action"].clone())
                        .unwrap_or(Value::Null),
                    Some(label.to_owned()),
                )),
            }
        }
        _ => Ok((
            array(&question["resolve"])
                .first()
                .map(|option| option["action"].clone())
                .unwrap_or(Value::Null),
            Some(label.to_owned()),
        )),
    }
}

fn per_round(plan: &Value) -> usize {
    plan["maxQuestionsPerRound"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(MAX_QUESTIONS_PER_ROUND)
}

/// Whether a question has to be answered, given the answers before it.
fn required(question: &Value, actions: &BTreeMap<String, Value>) -> bool {
    array(&question["when"])
        .iter()
        .all(|condition| actions.get(text(&condition["question"])) == Some(&condition["action"]))
}