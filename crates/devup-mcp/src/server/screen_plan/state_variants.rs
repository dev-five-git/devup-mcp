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
            .map(|frame| json!({"nodeId": frame["nodeId"], "name": frame["name"], "width": frame["width"]}))
            .collect::<Vec<_>>();
        if !states.is_empty() {
            route["states"] = json!(states);
        }
    }
}