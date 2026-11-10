fn validate_motion(value: Option<&Value>) -> Result<(), DevupError> {
    let value = value.ok_or_else(|| invalid("motion check requires a motion configuration"))?;
    let kind = value["kind"].as_str().unwrap_or_default();
    bounded_text(
        "motion.candidateId",
        value["candidateId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| invalid("motion needs its exact behavior candidateId"))?,
    )?;
    if !matches!(kind, "marquee" | "count-up" | "steps" | "tabs" | "reveal") {
        return Err(invalid(
            "motion.kind must be marquee, count-up, steps, tabs or reveal",
        ));
    }
    for key in match kind {
        "marquee" => vec!["target", "track"],
        "steps" => vec!["target", "steps", "progress"],
        "tabs" => vec!["target", "links", "indicator", "next"],
        _ => vec!["target"],
    } {
        let selector = value[key]
            .as_str()
            .ok_or_else(|| invalid(format!("motion.{key} requires a semantic selector")))?;
        bounded_text(&format!("motion.{key}"), selector)?;
        if selector.is_empty()
            || selector.contains("data-figma")
            || selector.contains("data-source-node")
        {
            return Err(invalid(
                "motion selectors must use application semantics, not source-node identities",
            ));
        }
    }
    if value
        .get("durationMs")
        .is_some_and(|value| value.as_u64().is_none_or(|ms| !(80..=10_000).contains(&ms)))
    {
        return Err(invalid("motion.durationMs must be 80..10000"));
    }
    if kind == "count-up"
        && value["finalValue"]
            .as_f64()
            .is_none_or(|number| !number.is_finite())
    {
        return Err(invalid("count-up motion needs finite finalValue"));
    }
    // Reveal scenarios each reload the page and replay one situation. They
    // are named exactly, so a typo cannot silently run nothing. With no list
    // every scenario runs.
    let semantic = |key: &str| -> Result<Option<&str>, DevupError> {
        let Some(selector) = value.get(key) else {
            return Ok(None);
        };
        let selector = selector
            .as_str()
            .filter(|selector| !selector.is_empty() && !selector.contains("data-figma") && !selector.contains("data-source-node"))
            .ok_or_else(|| invalid(format!("motion.{key} requires a semantic selector")))?;
        bounded_text(&format!("motion.{key}"), selector)?;
        Ok(Some(selector))
    };
    // What a reveal alone accepts: the animated items of a group, the
    // progress line, the control to focus and the link to leave the page by.
    let reveal_only = ["scenarios", "items", "progress", "focus", "navigate", "settleMs", "maxStepMs", "maxSpanMs", "maxTotalMs", "syncToleranceMs", "progressMs"];
    if kind != "reveal" && let Some(key) = reveal_only.iter().find(|key| value.get(**key).is_some()) {
        return Err(invalid(format!("motion.{key} is for reveal only")));
    }
    let (items, progress, _focus, navigate) = (semantic("items")?, semantic("progress")?, semantic("focus")?, semantic("navigate")?);
    if let Some(scenarios) = value.get("scenarios") {
        let valid = scenarios.as_array().is_some_and(|items| {
            items.len() <= REVEAL_SCENARIOS.len()
                && items.iter().all(|item| item.as_str().is_some_and(|name| REVEAL_SCENARIOS.contains(&name)))
        });
        if !valid {
            return Err(invalid(format!("motion.scenarios takes {}", REVEAL_SCENARIOS.join(", "))));
        }
        // A scenario named explicitly must be observable: its selectors are
        // required, never guessed.
        let named = |name: &str| scenarios.as_array().is_some_and(|items| items.iter().any(|item| item == name));
        for (scenario, key, present) in [
            ("progress-sync", "progress", progress.is_some()),
            ("progress-sync", "items", items.is_some()),
            ("stagger", "items", items.is_some()),
            ("client-nav", "navigate", navigate.is_some()),
        ] {
            if named(scenario) && !present {
                return Err(invalid(format!("{scenario} requires a semantic motion.{key} selector")));
            }
        }
    }
    for (key, max) in [("settleMs", 10_000), ("maxStepMs", 10_000), ("maxSpanMs", 60_000), ("maxTotalMs", 60_000), ("syncToleranceMs", 10_000), ("progressMs", 60_000)] {
        if value.get(key).is_some_and(|value| value.as_u64().is_none_or(|ms| ms > max)) {
            return Err(invalid(format!("motion.{key} must be 0..{max}")));
        }
    }
    if value.get("resizeWidth").is_some_and(|value| {
        value
            .as_u64()
            .is_none_or(|width| !(1..=4096).contains(&width))
    }) {
        return Err(invalid("motion.resizeWidth must be 1..4096"));
    }
    Ok(())
}

#[cfg(test)]
mod motion_contract_tests {
    use super::*;
    #[test]
    fn motion_probes_need_candidate_identity_semantics_and_bounded_measurements() {
        let valid = json!({"kind":"count-up","candidateId":"count-up:stat","target":"#patent-count","finalValue":2000,"durationMs":1600});
        assert!(validate_motion(Some(&valid)).is_ok());
        for patch in [
            json!({"candidateId":""}),
            json!({"target":"[data-figma-node-id]"}),
            json!({"durationMs":100_001}),
            json!({"finalValue":null}),
            json!({"kind":"unknown"}),
        ] {
            let mut changed = valid.clone();
            for (key, value) in patch.as_object().unwrap() {
                changed[key] = value.clone();
            }
            assert!(validate_motion(Some(&changed)).is_err(), "{changed}");
        }
        assert!(validate_motion(None).is_err());
        let reveal = json!({"kind":"reveal","candidateId":"section-entrance:1","target":"#news","scenarios":["fast-scroll","reload","history","bfcache","image-delay","image-failure","focus","reduced-motion","no-script"],"focus":"a[href]","settleMs":400});
        assert!(validate_motion(Some(&reveal)).is_ok());
        // No list: every scenario runs, and the selectors that some of them
        // need are optional (the scenario is then not-applicable or unverified).
        let every = json!({"kind":"reveal","candidateId":"section-entrance:1","target":"#news","navigate":"nav a[href='/about']"});
        assert!(validate_motion(Some(&every)).is_ok());
        let timeline = json!({"kind":"reveal","candidateId":"section-entrance:2","target":"#history","scenarios":["progress-sync","stagger"],"progress":".line","items":"li","maxStepMs":400,"maxSpanMs":900,"maxTotalMs":1400,"syncToleranceMs":150});
        assert!(validate_motion(Some(&timeline)).is_ok());
        let group = json!({"kind":"reveal","candidateId":"section-entrance:3","target":"#cards","items":".card","scenarios":["stagger","client-nav"],"navigate":"#about"});
        assert!(validate_motion(Some(&group)).is_ok());
        assert_eq!(REVEAL_SCENARIOS.len(), 12);
        for patch in [
            json!({"scenarios":["fast-scrol"]}),
            json!({"scenarios":["progress-sync"]}),
            json!({"scenarios":["progress-sync"],"progress":".line","items":"[data-figma-node-id]"}),
            json!({"scenarios":["stagger"]}),
            json!({"scenarios":["client-nav"]}),
            json!({"scenarios":"focus"}),
            json!({"kind":"tabs","links":"a","indicator":"i","next":"a"}),
            json!({"focus":"[data-figma-node-id]"}),
            json!({"navigate":"[data-source-node]"}),
            json!({"items":""}),
            json!({"settleMs":60_000}),
            json!({"maxStepMs":-1}),
            json!({"maxTotalMs":600_000}),
        ] {
            let mut changed = reveal.clone();
            for (key, value) in patch.as_object().unwrap() {
                changed[key] = value.clone();
            }
            assert!(validate_motion(Some(&changed)).is_err(), "{changed}");
        }
    }
}
