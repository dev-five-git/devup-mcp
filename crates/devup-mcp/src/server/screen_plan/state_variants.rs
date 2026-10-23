/// The answers to one structure question: keep as is, fix the Figma and, for
/// frames that may be one screen's states, map them to one URL.
fn structure_choices(oddity: &Oddity) -> Vec<Choice> {
    let keep = choice(
        oddity.keep,
        oddity.keep_description.clone(),
        json!({"structure": "keep"}),
    );
    let fix = choice(
        FIX_FIGMA,
        "These frames are not built. They are listed under Design fixes requested; everything else goes on.",
        json!({"designFix": true}),
    );
    let Some(recommended) = oddity.states else {
        return vec![keep, fix];
    };
    let states = choice(
        STATES,
        "One URL; the other frames are recorded as states of the first (empty, error, done, view switch) and audited per state.",
        json!({"structure": "states"}),
    );
    if recommended {
        vec![states, keep, fix]
    } else {
        vec![keep, states, fix]
    }
}

/// Per answered `states` question: the first frame's node id and the frames
/// folded into its route as states.
fn state_folds(
    questions: &[Value],
    actions: &BTreeMap<String, Value>,
) -> Vec<(String, Vec<Value>)> {
    questions
        .iter()
        .filter(|question| {
            actions
                .get(text(&question["id"]))
                .is_some_and(|action| action["structure"] == "states")
        })
        .filter_map(|question| {
            let frames = array(&question["fix"]["frames"]);
            let (first, rest) = frames.split_first()?;
            Some((text(&first["nodeId"]).to_owned(), rest.to_vec()))
        })
        .collect()
}

fn folded_ids(folds: &[(String, Vec<Value>)]) -> BTreeSet<String> {
    folds
        .iter()
        .flat_map(|(_, rest)| rest.iter().map(|frame| text(&frame["nodeId"]).to_owned()))
        .collect()
}

/// Records the folded frames on the route of the screen they belong to.
fn record_states(routes: &mut [Value], folds: &[(String, Vec<Value>)]) {
    for route in routes.iter_mut() {
        let states = folds
            .iter()
            .filter(|(first, _)| *first == text(&route["screen"]))
            .flat_map(|(_, rest)| rest.iter())
            .map(|frame| json!({"nodeId": frame["nodeId"], "name": frame["name"], "width": frame["width"], "height": frame["height"]}))
            .collect::<Vec<_>>();
        if !states.is_empty() {
            route["states"] = json!(states);
        }
    }
}

pub(super) fn state_exports(route: &Value) -> Vec<Value> {
    array(&route["states"]).iter().map(|state| {
        let mut state = state.clone();
        state["hostUrl"] = route["url"].clone();
        state["exportCall"] = json!({"tool": "devup_figma_export", "arguments": {
            "frameIds": [state["nodeId"]], "outputs": ["tsx", "referencePng"]
        }});
        state
    }).collect()
}

pub(super) fn audit_routes(plan: &Value) -> Vec<Value> {
    array(&plan["routes"]).iter().flat_map(|route| {
        let mut targets = vec![route.clone()];
        for state in array(&route["states"]) {
            let mut target = route.clone();
            target["nodeIds"] = json!([state["nodeId"]]);
            target["drawnWidths"] = json!([state["width"].as_f64().map(|width| width.round() as u64)]);
            target["state"] = state.clone();
            target["states"] = json!([]);
            targets.push(target);
        }
        targets
    }).collect()
}

fn distinct_recommended_routes(screens: &[Value], questions: &mut [Value]) {
    let mut ordered = questions.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|question| depth(question, questions));
    let mut actions = BTreeMap::new();
    for question in ordered {
        if required(question, &actions) {
            actions.insert(text(&question["id"]).to_owned(), question["resolve"][0]["action"].clone());
        }
    }
    let folded = folded_ids(&state_folds(questions, &actions));
    let mut placements = Vec::new();
    for screen in screens {
        let id = text(&screen["id"]);
        if folded.contains(id) || array(&screen["structureQuestions"]).iter().any(|question| actions.get(text(question)).is_some_and(|action| action["designFix"] == true)) {
            continue;
        }
        if let Some(page) = actions.get("base-page").and_then(|action| action["pageId"].as_str())
            && !array(&screen["frames"]).iter().any(|frame| frame["pageId"] == page) { continue; }
        let separate = actions.get(text(&screen["groupingQuestion"])).is_some_and(|action| action["group"] == "separate");
        let ids = if separate {
            array(&screen["frames"]).iter().map(|frame| format!("split-route:{}", text(&frame["nodeId"]))).collect::<Vec<_>>()
        } else {
            ["route", "modal", "review"].iter().map(|prefix| format!("{prefix}:{id}")).filter(|id| actions.contains_key(id)).take(1).collect()
        };
        if ids.is_empty() {
            if let Some(route) = screen["proposedRoute"].as_str() { placements.push((screen, None, route.to_owned())); }
        } else {
            for id in ids {
                if let Some(route) = actions[&id]["route"].as_str() { placements.push((screen, Some(id), route.to_owned())); }
            }
        }
    }
    let mut taken = BTreeSet::new();
    placements.sort_by_key(|(_, question, _)| question.is_some());
    for (screen, question_id, route) in placements {
        let app = text(&screen["app"]).to_owned();
        if taken.insert((app.clone(), route.clone())) { continue; }
        let Some(id) = question_id else { continue; };
        let mut suggestion = suggested_route(text(&screen["name"]), text(&screen["id"]), &route);
        while !taken.insert((app.clone(), suggestion.clone())) {
            suggestion.push_str(&format!("-{}", text(&screen["id"]).replace(':', "-")));
        }
        let asked = questions.iter_mut().find(|question| question["id"] == id).expect("placement question exists");
        let mut choices = vec![choice(&suggestion, format!("Distinct suggested URL: {route} is already recommended for another screen in this app."), json!({"route":suggestion}))];
        choices.extend(array(&asked["resolve"]).iter().zip(array(&asked["options"])).map(|(resolve, option)| choice(text(&resolve["label"]), text(&option["description"]), resolve["action"].clone())));
        let replacement = question(id, text(&asked["header"]).to_owned(), text(&asked["question"]).to_owned(), choices, text(&asked["kind"]));
        asked["resolve"] = replacement["resolve"].clone();
        asked["options"] = replacement["options"].clone();
    }
}
