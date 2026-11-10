// Motion evidence, as an agent reads it after ingest: one row per motion check
// per width, and one line per candidate saying whether it is verified.
//
// A candidate is verified only when a check for it passed at a mobile and at
// a desktop width. Everything else is listed as unverified with the reason:
// a width never run, a scenario not requested, a scenario that could not be
// observed. Unverified is never reported as a pass.

/// Widths below this are a mobile layout; at or above [`DESKTOP_MIN_WIDTH`]
/// a desktop one.
const MOBILE_MAX_WIDTH: u32 = 767;
const DESKTOP_MIN_WIDTH: u32 = 1024;

const REVEAL_SCENARIOS: [&str; 12] = [
    "fast-scroll", "reload", "history", "bfcache", "client-nav", "image-delay", "image-failure",
    "focus", "reduced-motion", "no-script", "stagger", "progress-sync",
];

fn viewport_class(width: u32) -> &'static str {
    if width <= MOBILE_MAX_WIDTH {
        "mobile"
    } else if width >= DESKTOP_MIN_WIDTH {
        "desktop"
    } else {
        "tablet"
    }
}

fn names(value: &Value) -> Vec<String> {
    value.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
}

/// One planned motion check at one width, with what the executor reported
/// for it. `item` is the executor's row; without one nothing ran.
struct MotionCheckAt<'a> {
    screen: &'a str,
    viewport: &'a ViewportInput,
    index: usize,
    check: &'a CheckInput,
    item: Option<&'a Value>,
    status: &'a str,
}

/// The evidence row of one motion check at one width. Without an executor
/// row every part of the check is unverified.
fn motion_evidence_row(policy: &OutputPolicy, report_dir: &Path, at: &MotionCheckAt<'_>) -> Value {
    let MotionCheckAt { screen, viewport, index, check, item, status } = *at;
    let motion = check.motion.as_ref().unwrap_or(&Value::Null);
    let kind = motion["kind"].as_str().unwrap_or_default();
    let measured = item.map_or(&Value::Null, |item| &item["measured"]);
    let mut row = json!({
        "screen": screen, "check": index,
        "viewport": {"width": viewport.width, "height": viewport.height, "class": viewport_class(viewport.width)},
        "candidateId": motion["candidateId"], "kind": kind, "status": status,
        "reason": item.map(|item| &item["reason"]),
    });
    if kind != "reveal" {
        return row;
    }
    let mut unverified = Vec::new();
    if item.is_none() || matches!(status, "not-run" | "skipped") {
        unverified.push("the whole check: it did not run at this width".to_owned());
    } else {
        // Frames: the values are the evidence; a screenshot is kept only when
        // its bytes match what the report says.
        let mut frames = serde_json::Map::new();
        for name in ["start", "middle", "end"] {
            let frame = &measured["frames"][name];
            if !frame.is_object() {
                unverified.push(format!("{name} frame"));
                continue;
            }
            let shot = &frame["screenshot"];
            let screenshot = shot.is_object().then(|| match verify_capture(policy, report_dir, shot, None) {
                Ok(_) => json!({"path": shot["path"], "verified": true}),
                Err(Integrity::Tampered(issue) | Integrity::Unverified(issue)) => json!({"path": shot["path"], "verified": false, "issue": issue}),
            });
            frames.insert(name.to_owned(), json!({
                "opacity": frame["opacity"], "translate": frame["translate"],
                "visibleRatio": frame["visibleRatio"], "screenshot": screenshot,
            }));
        }
        row["mode"] = measured["mode"].clone();
        row["frames"] = Value::Object(frames);
        let trigger = &measured["trigger"];
        row["trigger"] = json!({
            "verified": trigger["verified"], "startedOffscreen": trigger["startedOffscreen"],
            "late": trigger["late"], "visibleRatio": trigger["visibleRatio"],
            "completeVisiblePx": trigger["completeVisiblePx"], "reason": trigger["reason"],
        });
        if trigger["verified"] != true {
            unverified.push("trigger position".to_owned());
        }
        row["reentry"] = measured["reentry"].clone();
        if measured["reentry"]["verified"] != true {
            unverified.push("re-entry".to_owned());
        }
        let scenarios = measured["scenarios"].as_object();
        row["scenarios"] = json!(scenarios.map(|scenarios| {
            scenarios.iter().map(|(name, result)| (name.clone(), result["status"].clone())).collect::<serde_json::Map<_, _>>()
        }));
        for name in REVEAL_SCENARIOS {
            match scenarios.and_then(|scenarios| scenarios.get(name)).map(|result| &result["status"]).and_then(Value::as_str) {
                Some("pass" | "fail" | "not-applicable") => {}
                Some(_) => unverified.push(format!("{name}: {}", measured["scenarios"][name]["reason"].as_str().unwrap_or("could not be observed"))),
                None => unverified.push(format!("{name}: not requested")),
            }
        }
        row["notApplicable"] = json!(names(&measured["notApplicable"]));
        let stagger = &measured["scenarios"]["stagger"];
        if stagger["stepsMs"].is_array() {
            row["stagger"] = json!({"stepsMs": stagger["stepsMs"], "spanMs": stagger["spanMs"], "totalMs": stagger["totalMs"], "bounds": stagger["bounds"]});
        }
        let sync = &measured["scenarios"]["progress-sync"];
        if sync["gaps"].is_array() {
            row["progressSync"] = json!({"gapsMs": sync["gaps"], "toleranceMs": sync["toleranceMs"], "axis": sync["axis"]});
        }
    }
    row["unverified"] = json!(unverified);
    row
}

/// Folds the rows into one line per candidate. `verified` is true only with
/// a passing row at a mobile and a desktop width and nothing left unverified
/// in those rows.
fn motion_evidence_summary(rows: Vec<Value>) -> Value {
    let mut ids: Vec<&str> = rows.iter().filter_map(|row| row["candidateId"].as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    let candidates = ids
        .iter()
        .map(|id| {
            let own: Vec<&Value> = rows.iter().filter(|row| row["candidateId"] == *id).collect();
            let passed = |class: &str| own.iter().any(|row| row["status"] == "pass" && row["viewport"]["class"] == class);
            let mut unverified: Vec<String> = Vec::new();
            for class in ["mobile", "desktop"] {
                if !passed(class) {
                    unverified.push(format!("no passing check at a {class} width"));
                }
            }
            for row in &own {
                for entry in names(&row["unverified"]) {
                    let entry = format!("{}px: {entry}", row["viewport"]["width"]);
                    if !unverified.contains(&entry) {
                        unverified.push(entry);
                    }
                }
            }
            let failed = own.iter().any(|row| row["status"] == "fail");
            json!({
                "candidateId": id, "kind": own[0]["kind"],
                "passedWidths": own.iter().filter(|row| row["status"] == "pass").map(|row| row["viewport"]["width"].clone()).collect::<Vec<_>>(),
                "failedWidths": own.iter().filter(|row| row["status"] == "fail").map(|row| row["viewport"]["width"].clone()).collect::<Vec<_>>(),
                "status": if failed { "fail" } else if unverified.is_empty() { "verified" } else { "unverified" },
                "unverified": unverified,
            })
        })
        .collect::<Vec<_>>();
    let count = |status: &str| candidates.iter().filter(|candidate| candidate["status"] == status).count();
    json!({
        "verified": count("verified"), "unverified": count("unverified"), "failed": count("fail"),
        "candidates": candidates,
        "rows": rows,
        "rule": "A motion candidate is verified only by a passing check at a mobile (<=767px) and a desktop (>=1024px) width with nothing left unverified. Scenarios that were not requested or could not be observed are unverified, never passed; not-applicable means the block has nothing for that scenario to observe.",
    })
}

#[cfg(test)]
mod motion_evidence_tests {
    use super::*;

    fn viewport(width: u32) -> ViewportInput {
        serde_json::from_value(json!({"width": width, "height": 800})).unwrap()
    }

    fn reveal_check() -> CheckInput {
        serde_json::from_value(json!({"kind": "motion", "motion": {"kind": "reveal", "candidateId": "section-entrance:news", "target": "#news"}})).unwrap()
    }

    /// What reveal-checks.mjs reports for a passing scroll-linked section.
    fn measured(scenarios: Value) -> Value {
        let frame = |opacity: f64, translate: f64, ratio: f64| json!({"opacity": opacity, "translate": translate, "visibleRatio": ratio});
        json!({"kind": "reveal", "candidateId": "section-entrance:news", "mode": "scroll-linked",
            "frames": {"start": frame(0.0, 16.0, 0.0), "middle": frame(0.24, 12.1, 0.15), "end": frame(1.0, 0.0, 1.0)},
            "trigger": {"verified": true, "startedOffscreen": false, "late": false, "visibleRatio": 0.01, "completeVisiblePx": 242},
            "reentry": {"verified": true, "replayed": false, "hiddenWhileAway": false},
            "scenarios": scenarios, "notApplicable": ["stagger", "progress-sync"], "unverified": []})
    }

    fn every_scenario() -> Value {
        let mut scenarios = serde_json::Map::new();
        for name in REVEAL_SCENARIOS {
            let status = if matches!(name, "stagger" | "progress-sync") { "not-applicable" } else { "pass" };
            scenarios.insert(name.to_owned(), json!({"status": status}));
        }
        Value::Object(scenarios)
    }

    fn row(width: u32, status: &str, measured: Option<Value>) -> Value {
        let policy = OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap();
        let item = measured.map(|measured| json!({"index": 0, "status": status, "reason": "observed", "measured": measured}));
        motion_evidence_row(&policy, &std::env::temp_dir(), &MotionCheckAt { screen: "home", viewport: &viewport(width), index: 0, check: &reveal_check(), item: item.as_ref(), status })
    }

    #[test]
    fn a_candidate_is_verified_only_by_passing_mobile_and_desktop_checks() {
        let rows = vec![row(390, "pass", Some(measured(every_scenario()))), row(1440, "pass", Some(measured(every_scenario())))];
        assert_eq!(rows[0]["viewport"]["class"], "mobile");
        assert_eq!(rows[1]["viewport"]["class"], "desktop");
        assert_eq!(rows[0]["frames"]["middle"]["opacity"], 0.24);
        assert_eq!(rows[0]["trigger"]["completeVisiblePx"], 242);
        assert_eq!(rows[0]["unverified"], json!([]));
        assert_eq!(rows[0]["notApplicable"], json!(["stagger", "progress-sync"]));
        let summary = motion_evidence_summary(rows);
        assert_eq!((summary["verified"].clone(), summary["unverified"].clone(), summary["failed"].clone()), (json!(1), json!(0), json!(0)));
        assert_eq!(summary["candidates"][0]["status"], "verified");
        assert_eq!(summary["candidates"][0]["passedWidths"], json!([390, 1440]));

        // Desktop only: the mobile width was never checked.
        let summary = motion_evidence_summary(vec![row(1440, "pass", Some(measured(every_scenario())))]);
        assert_eq!(summary["candidates"][0]["status"], "unverified");
        assert_eq!(summary["candidates"][0]["unverified"], json!(["no passing check at a mobile width"]));
        // A tablet width is neither.
        let summary = motion_evidence_summary(vec![row(858, "pass", Some(measured(every_scenario())))]);
        assert_eq!(summary["candidates"][0]["unverified"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn what_was_not_requested_or_not_observed_is_unverified_never_a_pass() {
        // Only two scenarios were requested, and one could not be observed.
        let partial = json!({"fast-scroll": {"status": "pass"}, "bfcache": {"status": "unverified", "reason": "no browser with the back/forward cache could be started"}});
        let mobile = row(390, "inconclusive", Some(measured(partial)));
        let unverified = mobile["unverified"].as_array().unwrap();
        assert!(unverified.contains(&json!("bfcache: no browser with the back/forward cache could be started")), "{mobile}");
        assert!(unverified.contains(&json!("reload: not requested")), "{mobile}");
        assert!(!unverified.iter().any(|entry| entry.as_str().unwrap().starts_with("fast-scroll")), "{mobile}");
        assert_eq!(mobile["scenarios"]["bfcache"], "unverified");

        // A width where the check never ran, and one with no frames or trigger.
        let missing = row(1440, "not-run", None);
        assert_eq!(missing["unverified"], json!(["the whole check: it did not run at this width"]));
        let mut bare = measured(every_scenario());
        bare["frames"] = json!({"end": {"opacity": 1, "translate": 0}});
        bare["trigger"] = json!({"verified": false, "reason": "target is already in view"});
        bare["reentry"] = json!({"verified": false});
        let bare = row(390, "inconclusive", Some(bare));
        assert_eq!(bare["unverified"], json!(["start frame", "middle frame", "trigger position", "re-entry"]));

        let summary = motion_evidence_summary(vec![mobile, missing]);
        assert_eq!(summary["candidates"][0]["status"], "unverified");
        assert_eq!(summary["verified"], 0);
        assert!(summary["candidates"][0]["unverified"].as_array().unwrap().iter().any(|entry| entry == "1440px: the whole check: it did not run at this width"));
        // A failure anywhere is a failure, whatever else passed.
        let summary = motion_evidence_summary(vec![row(390, "pass", Some(measured(every_scenario()))), row(1440, "fail", Some(measured(every_scenario())))]);
        assert_eq!(summary["candidates"][0]["status"], "fail");
        assert_eq!(summary["candidates"][0]["failedWidths"], json!([1440]));
    }

    #[test]
    fn frame_screenshots_and_timing_measurements_are_carried_as_evidence() {
        let dir = std::env::temp_dir().join(format!("devup-motion-evidence-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let image = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 255, 255, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        let bytes = bytes.into_inner();
        std::fs::write(dir.join("frame.png"), &bytes).unwrap();
        let mut scenarios = every_scenario();
        scenarios["stagger"] = json!({"status": "pass", "stepsMs": [50, 67, 50], "spanMs": 167, "totalMs": 567, "bounds": {"maxStepMs": 150}});
        scenarios["progress-sync"] = json!({"status": "pass", "gaps": [10, -20, 5], "toleranceMs": 200, "axis": "vertical"});
        let mut measured = measured(scenarios);
        measured["frames"]["start"]["screenshot"] = json!({"path": "frame.png", "sha256": sha256(&bytes), "bytes": bytes.len()});
        measured["frames"]["end"]["screenshot"] = json!({"path": "frame.png", "sha256": "0".repeat(64), "bytes": bytes.len()});
        let policy = OutputPolicy::from_roots(vec![dir.clone()]).unwrap();
        let item = json!({"index": 0, "status": "pass", "measured": measured});
        let row = motion_evidence_row(&policy, &dir, &MotionCheckAt { screen: "home", viewport: &viewport(390), index: 0, check: &reveal_check(), item: Some(&item), status: "pass" });
        drop(policy);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(row["frames"]["start"]["screenshot"], json!({"path": "frame.png", "verified": true}));
        // Bytes that differ from the report are not evidence.
        assert_eq!(row["frames"]["end"]["screenshot"]["verified"], false);
        assert_eq!(row["frames"]["middle"]["screenshot"], Value::Null);
        assert_eq!(row["stagger"]["totalMs"], 567);
        assert_eq!(row["progressSync"]["gapsMs"], json!([10, -20, 5]));
        // Other motion kinds keep their own row without reveal fields.
        let tabs: CheckInput = serde_json::from_value(json!({"kind": "motion", "motion": {"kind": "tabs", "candidateId": "tabs:1", "target": "nav", "links": "a", "indicator": "i", "next": "a"}})).unwrap();
        let item = json!({"status": "pass", "measured": {}});
        let row = motion_evidence_row(&OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap(), &std::env::temp_dir(), &MotionCheckAt { screen: "home", viewport: &viewport(390), index: 1, check: &tabs, item: Some(&item), status: "pass" });
        assert!(row.get("unverified").is_none() && row.get("frames").is_none(), "{row}");
    }
}
