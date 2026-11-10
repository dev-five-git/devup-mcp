use super::*;

pub(super) fn attach(result: &mut Value) {
    let candidates = result["behaviorPlan"]["candidates"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let bindings = candidates.iter().filter_map(|candidate| {
        let kind = match candidate["kind"].as_str()? {
            "count-up" => "count-up", "marquee" => "marquee", "progress-step-cycle" => "progress-step-cycle",
            "sliding-tab-indicator" => "sliding-route-highlight", "section-entrance" => "section-entrance", _ => return None,
        };
        Some(json!({"candidateId":candidate["id"],"binding":{"kind":kind,"target":null,
            "value":candidate["evidence"]["target"],"precision":candidate["evidence"]["precision"],"unit":candidate["evidence"]["unit"],"prefix":candidate["evidence"]["prefix"]},
            "requiresBinding":true,"implementedController":true,"wired":false,"runtimeVerified":false,
            "requiredSelectors":match kind {"marquee"=>vec!["target","track","group"],"progress-step-cycle"=>vec!["target","steps","panels","progress"],"sliding-route-highlight"=>vec!["target","links","indicator"],_=>vec!["target"]}}))
    }).collect::<Vec<_>>();
    if bindings.is_empty() {
        return;
    }
    result["motionControllerTsx"] = json!(include_str!("templates/MotionController.tsx"));
    result["motionRuntime"] = json!(include_str!("templates/motion-runtime.ts"));
    // Entrances are static CSS: the component that declares the keyframes
    // ships only when the plan found a section to reveal.
    let reveals = bindings.iter().any(|binding| binding["binding"]["kind"] == "section-entrance");
    if reveals {
        result["revealTsx"] = json!(include_str!("templates/Reveal.tsx"));
    }
    result["motionBindings"] = json!({"version":1,"bindings":bindings,
        "integration":"Keep generated server TSX. Bind semantic selectors and validated API values; mount MotionController once in the persistent server layout with those bindings. Keep the original captured palette/indicator/progress markup. Selectors are required, never guessed from node IDs. A section entrance is static CSS: wrap the section's content group (not the painted section) in Reveal, or RevealGroup with RevealItem for staggered cards and RevealLine/RevealStep for a progress line, and bind section-entrance to [data-reveal].",
        "files":if reveals { json!({"motionControllerTsx":"MotionController.tsx","motionRuntime":"motion-runtime.ts","revealTsx":"Reveal.tsx"}) } else { json!({"motionControllerTsx":"MotionController.tsx","motionRuntime":"motion-runtime.ts"}) },
        "entranceStyle":"devup-ui-static-keyframes",
        "playback":"Continuous marquee/steps have no pause controls; set respectReducedMotion:false only for an explicit continuous-playback requirement.",
        "effectReason":"One effect attaches page DOM observers and cancels them at unmount; layouts and data stay server-rendered.",
        "implemented":true,"wired":false,"runtimeVerified":false});
    result["behaviorPlan"]["motionImplementations"] = result["motionBindings"].clone();
}
