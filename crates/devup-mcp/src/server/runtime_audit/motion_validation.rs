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
    // are named exactly, so a typo cannot silently run nothing.
    if let Some(scenarios) = value.get("scenarios") {
        let known = ["fast-scroll", "reload", "history", "image-delay", "image-failure", "focus", "progress-sync"];
        let valid = kind == "reveal"
            && scenarios.as_array().is_some_and(|items| {
                items.len() <= known.len()
                    && items.iter().all(|item| item.as_str().is_some_and(|name| known.contains(&name)))
            });
        if !valid {
            return Err(invalid(
                "motion.scenarios is for reveal only and takes fast-scroll, reload, history, image-delay, image-failure, focus or progress-sync",
            ));
        }
        // The line and its items are named, never guessed.
        if scenarios.as_array().is_some_and(|items| items.iter().any(|item| item == "progress-sync")) {
            for key in ["progress", "items"] {
                let selector = value[key]
                    .as_str()
                    .filter(|selector| !selector.is_empty() && !selector.contains("data-figma") && !selector.contains("data-source-node"))
                    .ok_or_else(|| invalid(format!("progress-sync requires a semantic motion.{key} selector")))?;
                bounded_text(&format!("motion.{key}"), selector)?;
            }
        }
    }
    if let Some(focus) = value.get("focus") {
        let selector = focus
            .as_str()
            .filter(|selector| !selector.is_empty() && !selector.contains("data-figma") && !selector.contains("data-source-node"))
            .ok_or_else(|| invalid("motion.focus requires a semantic selector"))?;
        bounded_text("motion.focus", selector)?;
    }
    if value
        .get("settleMs")
        .is_some_and(|value| value.as_u64().is_none_or(|ms| ms > 10_000))
    {
        return Err(invalid("motion.settleMs must be 0..10000"));
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
        let reveal = json!({"kind":"reveal","candidateId":"section-entrance:1","target":"#news","scenarios":["fast-scroll","reload","history","image-delay","image-failure","focus"],"focus":"a[href]","settleMs":400});
        assert!(validate_motion(Some(&reveal)).is_ok());
        let timeline = json!({"kind":"reveal","candidateId":"section-entrance:2","target":"#history","scenarios":["progress-sync"],"progress":".line","items":"li"});
        assert!(validate_motion(Some(&timeline)).is_ok());
        for patch in [
            json!({"scenarios":["fast-scrol"]}),
            json!({"scenarios":["progress-sync"]}),
            json!({"scenarios":["progress-sync"],"progress":".line","items":"[data-figma-node-id]"}),
            json!({"scenarios":"focus"}),
            json!({"kind":"tabs","links":"a","indicator":"i","next":"a"}),
            json!({"focus":"[data-figma-node-id]"}),
            json!({"settleMs":60_000}),
        ] {
            let mut changed = reveal.clone();
            for (key, value) in patch.as_object().unwrap() {
                changed[key] = value.clone();
            }
            assert!(validate_motion(Some(&changed)).is_err(), "{changed}");
        }
    }
}
