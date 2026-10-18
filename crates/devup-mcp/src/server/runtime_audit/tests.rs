

#[cfg(test)]
mod tests {
    use super::*;

    async fn dimension_fixture() -> (ArtifactStore, String) {
        use devup_mcp_figma::{CollectedPayload, CollectionRequest, CollectionScope, FigmaTarget};
        let store = ArtifactStore::default();
        let target = FigmaTarget {
            file_key: "synthetic".into(),
            node_id: Some("target".into()),
            branch_key: None,
        };
        let request = CollectionRequest::new(target.clone(), CollectionScope::Node);
        let payload: CollectedPayload = serde_json::from_value(json!({
            "target": target, "scope": "node", "metadata": {},
            "snapshot": {"fileKey": "synthetic", "version": "1", "roots": ["target"],
                "nodes": {"target": {"id": "target", "type": "FRAME", "fields": {"width": 100, "height": 80}},
                    "bounds-only": {"id": "bounds-only", "type": "FRAME", "fields": {"absoluteBoundingBox": {"width": 100, "height": 80}}},
                    "errored": {"id": "errored", "type": "FRAME", "fields": {"width": 100, "height": 80}, "fieldErrors": {"width": "unavailable"}}}, "diagnostics": []},
            "variables": null, "styles": null, "completeness": "resolved-values-only", "sourceVersion": "1"
        })).unwrap();
        let artifact = store
            .get_or_acquire(
                super::super::artifacts::ArtifactRequestKey::from_collection(&request),
                false,
                || async { Ok(payload) },
            )
            .await
            .unwrap();
        (store, artifact.artifact_id)
    }

    fn dimensions_plan(artifact: &str) -> PlanInput {
        let widths = [1440, 1100, 858, 390];
        let mappings = widths
            .iter()
            .map(
                |width| json!({"viewportWidth": width, "artifactId": artifact, "nodeId": "target"}),
            )
            .collect::<Vec<_>>();
        serde_json::from_value(json!({
            "baseUrl": "http://127.0.0.1:4173", "allowedOrigins": ["http://127.0.0.1:4173"],
            "viewports": widths.iter().map(|width| json!({"width": width, "height": 100, "deviceScaleFactor": 2})).collect::<Vec<_>>(),
            "screens": [{"id": "dimensions", "routePath": "/", "checks": [
                {"kind": "dimensions", "subject": {"testId": "target"}, "tolerancePx": 0.5, "figmaDimensions": mappings},
                {"kind": "dimensions", "subject": {"testId": "target"}, "tolerancePx": 0.49, "figmaDimensions": mappings}
            ]}]
        })).unwrap()
    }

    #[tokio::test]
    async fn acquired_dimensions_are_rechecked_and_tolerances_recomputed_at_all_widths() {
        let (dir, policy) = root();
        let (store, artifact) = dimension_fixture().await;
        let planned = create_plan(
            dimensions_plan(&artifact),
            &policy,
            Some("plan.json"),
            Some("runner"),
            &store,
        )
        .await
        .unwrap();
        assert_eq!(planned["template"]["files"].as_array().unwrap().len(), 5);
        assert_eq!(planned["template"]["executed"], false);
        assert_eq!(
            std::fs::read(dir.join("runner/run.mjs")).unwrap(),
            include_bytes!("../../../../../harness/runtime-audit/run.mjs")
        );
        let plan_bytes = std::fs::read(dir.join("plan.json")).unwrap();
        let mut report = json!({"schemaVersion": 1, "kind": REPORT_KIND, "planId": planned["planId"],
            "planSha256": sha256(&plan_bytes), "status": "completed", "runs": []});
        for width in [1440_u32, 1100, 858, 390] {
            let bytes = png(width * 2, 200);
            let name = format!("{width}.png");
            std::fs::write(dir.join(&name), &bytes).unwrap();
            report["runs"].as_array_mut().unwrap().push(json!({
                "viewport": {"width": width, "height": 100, "deviceScaleFactor": 2}, "environmentErrors": [],
                "screens": [{"id": "dimensions", "status": "completed",
                    "checks": (0..2).map(|index| json!({"index": index, "status": "pass", "measured": {
                        "units": "css-px", "subjectCount": 1, "subjectVisible": true,
                        "subject": {"x": 0, "y": 0, "width": 100.5, "height": 80, "extra": "untrusted-subject-extension"}}})).collect::<Vec<_>>(),
                    "screenshot": {"path": name, "sha256": sha256(&bytes), "bytes": bytes.len()}}]
            }));
        }
        let write = |value: &Value| {
            std::fs::write(dir.join("report.json"), serde_json::to_vec(value).unwrap()).unwrap()
        };
        write(&report);
        let result = ingest_with_artifacts(&policy, "plan.json", "report.json", &store)
            .await
            .unwrap();
        assert_eq!(result["verdict"], "fail", "{result}");
        assert_eq!(result["summary"]["pass"], 4);
        assert_eq!(result["summary"]["fail"], 4);
        assert!(!result.to_string().contains("untrusted-subject-extension"));
        assert_eq!(result["toleranceExceedances"].as_array().unwrap().len(), 4);
        for row in result["widthTable"].as_array().unwrap() {
            assert_eq!(row["dimensions"][0]["deltaPx"]["width"], 0.5);
            assert_eq!(
                row["dimensions"][0]["referenceVerifiedAgainstRetainedAcquisition"],
                true
            );
            assert_eq!(row["dimensions"][1]["executorOutcomeMatches"], false);
        }

        let mut ambiguous = report.clone();
        let repeated = ambiguous["runs"][0]["screens"][0]["checks"][0].clone();
        ambiguous["runs"][0]["screens"][0]["checks"]
            .as_array_mut()
            .unwrap()
            .push(repeated);
        write(&ambiguous);
        assert_eq!(
            ingest_with_artifacts(&policy, "plan.json", "report.json", &store)
                .await
                .unwrap()["reason"],
            "ambiguous-report-items"
        );

        let mut missing = report.clone();
        missing["runs"].as_array_mut().unwrap().pop();
        missing["runs"][0]["screens"][0]["checks"][0]["status"] = json!("skipped");
        write(&missing);
        let result = ingest_with_artifacts(&policy, "plan.json", "report.json", &store)
            .await
            .unwrap();
        assert_eq!(result["verdict"], "inconclusive");
        assert_eq!(result["summary"]["notRun"], 2);
        assert_eq!(result["summary"]["skipped"], 1);
        assert_eq!(
            result["widthTable"][3]["dimensions"][0]["status"],
            "not-run"
        );

        write(&report);
        let empty_store = ArtifactStore::default();
        let result = ingest_with_artifacts(&policy, "plan.json", "report.json", &empty_store)
            .await
            .unwrap();
        assert_eq!(result["summary"]["inconclusive"], 8);
        assert_eq!(result["toleranceExceedances"], json!([]));
        assert_eq!(
            result["widthTable"][0]["dimensions"][0]["referenceVerifiedAgainstRetainedAcquisition"],
            false
        );

        for (field, value) in [
            ("width", json!(100.5)),
            ("height", json!(80.5)),
            ("status", json!("caller-asserted")),
            ("artifactContentHash", json!("forged")),
        ] {
            let mut forged = planned.clone();
            forged["dimensionReferences"][0][field] = value;
            let forged_bytes = serde_json::to_vec(&forged).unwrap();
            std::fs::write(dir.join("plan.json"), &forged_bytes).unwrap();
            let mut rebound = report.clone();
            rebound["planSha256"] = json!(sha256(&forged_bytes));
            write(&rebound);
            let result = ingest_with_artifacts(&policy, "plan.json", "report.json", &store)
                .await
                .unwrap();
            assert_eq!(
                result["widthTable"][0]["dimensions"][0]["status"], "inconclusive",
                "{field}"
            );
        }

        std::fs::write(dir.join("plan.json"), &plan_bytes).unwrap();
        write(&report);
        std::fs::write(dir.join("1440.png"), png(2880, 201)).unwrap();
        assert_eq!(
            ingest_with_artifacts(&policy, "plan.json", "report.json", &store)
                .await
                .unwrap()["reason"],
            "artifact-changed"
        );
        drop(policy);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn dimension_references_and_template_outputs_refuse_unverified_or_unsafe_inputs() {
        let (dir, policy) = root();
        let (store, artifact) = dimension_fixture().await;
        let input = dimensions_plan(&artifact);
        let mut oversized = input.clone();
        oversized.screens[0].checks =
            vec![input.screens[0].checks[0].clone(); MAX_DIMENSION_ROWS / 4 + 1];
        assert!(validate_plan(&oversized).is_err());
        let mut defaults = serde_json::to_value(&input).unwrap();
        defaults.as_object_mut().unwrap().remove("viewports");
        assert_eq!(
            serde_json::from_value::<PlanInput>(defaults)
                .unwrap()
                .viewports
                .iter()
                .map(|v| v.width)
                .collect::<Vec<_>>(),
            vec![1440, 1100, 858, 390]
        );
        for node_id in ["bounds-only", "errored", "absent"] {
            let mut missing = input.clone();
            missing.screens[0].checks[0].figma_dimensions[0].node_id = node_id.into();
            let value = create_plan(missing, &policy, None, None, &store)
                .await
                .unwrap();
            assert_eq!(
                value["dimensionReferences"][0]["status"], "unverified",
                "{node_id}"
            );
        }
        for tolerance in [-0.1, 10000.1] {
            let mut invalid = input.clone();
            invalid.screens[0].checks[0].tolerance_px = Some(tolerance);
            assert!(validate_plan(&invalid).is_err());
        }
        let mut duplicate = input.clone();
        let mapping = duplicate.screens[0].checks[0].figma_dimensions[0].clone();
        duplicate.screens[0].checks[0]
            .figma_dimensions
            .push(mapping);
        assert!(validate_plan(&duplicate).is_err());
        for (output, template) in [
            (Some("safe.json"), Some("../escape")),
            (Some("../escape.json"), Some("safe")),
            (None, Some("safe")),
        ] {
            assert!(
                create_plan(input.clone(), &policy, output, template, &store)
                    .await
                    .is_err()
            );
            assert!(!dir.join("safe.json").exists());
            assert!(!dir.join("safe/run.mjs").exists());
        }
        std::fs::create_dir_all(dir.join("existing")).unwrap();
        std::fs::write(dir.join("existing/checks.mjs"), b"keep").unwrap();
        assert!(
            create_plan(input, &policy, Some("safe.json"), Some("existing"), &store)
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read(dir.join("existing/checks.mjs")).unwrap(),
            b"keep"
        );
        assert!(!dir.join("safe.json").exists());
        assert!(!dir.join("existing/run.mjs").exists());
        drop(policy);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn plan_input() -> PlanInput {
        serde_json::from_value(json!({
            "baseUrl": "http://127.0.0.1:4173",
            "allowedOrigins": ["http://127.0.0.1:4173"],
            "viewports": [{"width": 360, "height": 740}, {"width": 1200, "height": 800}],
            "screens": [
                {"id": "today", "routePath": "/today",
                 "figma": {"nodeId": "1:138", "drawnWidths": [360]},
                 "checks": [
                    {"kind": "no-horizontal-overflow"},
                    {"kind": "contained", "subject": {"testId": "badge-text"}, "container": {"testId": "badge"}}
                 ]},
                {"id": "next", "steps": [{"action": "click", "target": {"role": "button", "name": "Next"}}],
                 "expect": {"urlPath": "/next"},
                 "checks": [{"kind": "aligned", "edge": "right", "offsetPx": 36,
                    "subject": {"testId": "qr"}, "container": {"testId": "card"}}]}
            ]
        }))
        .unwrap()
    }

    fn root() -> (std::path::PathBuf, OutputPolicy) {
        let dir = std::env::temp_dir().join(format!(
            "devup-runtime-audit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let policy = OutputPolicy::from_roots(vec![dir.clone()]).unwrap();
        (dir, policy)
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba([255, 255, 255, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    fn report_for(
        dir: &Path,
        plan_file: &[u8],
        id: &str,
        check_status: &str,
        height_1200: u32,
    ) -> Value {
        let mut runs = Vec::new();
        for (width, height) in [(360_u32, 740_u32), (1200, height_1200)] {
            let mut screens = Vec::new();
            for (screen, checks) in [("today", 2), ("next", 1)] {
                let name = format!("{width}-{screen}.png");
                let bytes = png(width, height.max(740));
                std::fs::write(dir.join(&name), &bytes).unwrap();
                screens.push(json!({"id": screen, "status": "completed",
                    "checks": (0..checks).map(|index| json!({"index": index, "status": check_status, "reason": "measured"})).collect::<Vec<_>>(),
                    "screenshot": {"path": name, "sha256": sha256(&bytes), "bytes": bytes.len()}}));
            }
            runs.push(json!({"viewport": {"width": width, "height": if width == 360 { 740 } else { 800 }, "deviceScaleFactor": 1},
                "environmentErrors": [], "screens": screens}));
        }
        json!({"schemaVersion": 1, "kind": REPORT_KIND, "planId": id, "planSha256": sha256(plan_file),
            "status": "completed", "runs": runs, "summary": {"pass": 6, "fail": 0, "inconclusive": 0, "skipped": 0}})
    }

    fn write_plan(dir: &Path, policy: &OutputPolicy) -> (String, Vec<u8>) {
        let document = plan(
            plan_input(),
            policy,
            Some(&dir.join("plan.json").to_string_lossy()),
        )
        .unwrap();
        let bytes = std::fs::read(dir.join("plan.json")).unwrap();
        (document["planId"].as_str().unwrap().to_owned(), bytes)
    }

    #[test]
    fn a_view_transition_check_needs_the_link_to_follow() {
        let mut plan = plan_input();
        plan.screens[0].checks.push(
            serde_json::from_value(json!({"kind": "view-transition", "subject": {"testId": "go"}}))
                .unwrap(),
        );
        assert!(validate_plan(&plan).is_ok());
        plan.screens[0]
            .checks
            .push(serde_json::from_value(json!({"kind": "view-transition"})).unwrap());
        let error = validate_plan(&plan).unwrap_err();
        assert!(
            error
                .message
                .contains("view-transition check takes a subject"),
            "{}",
            error.message
        );
    }

    #[test]
    fn the_new_check_kinds_take_what_they_measure() {
        let ok = |check: Value| {
            let mut plan = plan_input();
            plan.screens[0]
                .checks
                .push(serde_json::from_value(check).unwrap());
            validate_plan(&plan).map_err(|error| error.message)
        };
        let node = |id: &str| json!({"css": format!("[data-figma-node-id=\"{id}\"]")});
        assert!(ok(json!({"kind": "page-health"})).is_ok());
        assert!(ok(json!({"kind": "no-clipped-text"})).is_ok());
        assert!(ok(json!({"kind": "interaction", "state": "hover"})).is_ok());
        assert!(
            ok(
                json!({"kind": "bounds", "subject": node("1:2"), "container": node("1:1"),
            "expected": {"x": 0, "y": 0, "width": 10, "height": 10}, "widths": [1440]})
            )
            .is_ok()
        );
        assert!(
            ok(json!({"kind": "capture", "subject": node("1:2"),
            "reference": {"scope": "asset", "artifactId": "a", "assetId": "b"}}))
            .is_ok()
        );
        assert!(
            ok(json!({"kind": "bounds", "subject": node("1:2"), "container": node("1:1")}))
                .unwrap_err()
                .contains("expected")
        );
        assert!(
            ok(json!({"kind": "interaction"}))
                .unwrap_err()
                .contains("hover or focus")
        );
        assert!(ok(json!({"kind": "hidden", "subject": node("1:9")})).is_ok());
        assert!(
            ok(
                json!({"kind": "bounds", "subject": node("1:2"), "container": node("1:1"),
                "expected": {"x": 0, "y": 0, "width": 10, "height": 10}, "tolerancePx": 4})
            )
            .unwrap_err()
            .contains("exact"),
            "geometry takes no tolerance"
        );
        assert!(
            ok(json!({"kind": "capture", "subject": node("1:2")}))
                .unwrap_err()
                .contains("reference")
        );
        assert!(
            ok(json!({"kind": "visible", "subject": node("1:2"), "state": "hover"}))
                .unwrap_err()
                .contains("only valid for interaction")
        );
    }

    #[test]
    fn view_transition_checks_move_behind_the_rest_of_their_screen() {
        let mut plan = plan_input();
        plan.screens[0].checks = serde_json::from_value(json!([
            {"kind": "view-transition", "subject": {"testId": "go"}, "label": "vt"},
            {"kind": "page-health", "label": "health"},
            {"kind": "no-horizontal-overflow", "label": "overflow"},
        ]))
        .unwrap();
        order_checks(&mut plan);
        let labels = plan.screens[0]
            .checks
            .iter()
            .map(|check| check.label.clone().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(labels, ["health", "overflow", "vt"]);
    }

    #[test]
    fn a_plan_is_identified_by_its_content_and_refuses_what_is_not_declarative() {
        let first = plan_id(&plan_input()).unwrap();
        assert_eq!(first, plan_id(&plan_input()).unwrap());
        let mut edited = plan_input();
        edited.viewports[0].width = 361;
        assert_ne!(first, plan_id(&edited).unwrap());

        let shell = serde_json::from_value::<PlanInput>(json!({
            "baseUrl": "http://127.0.0.1:4173", "allowedOrigins": ["http://127.0.0.1:4173"],
            "viewports": [{"width": 360, "height": 740}],
            "screens": [{"id": "a", "routePath": "/", "command": "rm -rf /"}]
        }));
        assert!(
            shell.is_err(),
            "unknown fields such as a command are refused"
        );

        for (field, value) in [
            ("baseUrl", json!("file:///etc/passwd")),
            ("allowedOrigins", json!(["http://evil.example"])),
            ("viewports", json!([{"width": 99999, "height": 740}])),
        ] {
            let mut raw = serde_json::to_value(plan_input()).unwrap();
            raw[field] = value;
            let parsed: PlanInput = serde_json::from_value(raw).unwrap();
            assert!(validate_plan(&parsed).is_err(), "{field} must be refused");
        }
        let mut css_step = plan_input();
        css_step.screens[1].steps[0].target = LocatorInput {
            css: Some("button".into()),
            ..LocatorInput::default()
        };
        assert!(
            validate_plan(&css_step).is_err(),
            "steps use role/testId/text"
        );

        let mut unverifiable = plan_input();
        unverifiable.viewports = vec![unverifiable.viewports[0].clone(); MAX_VIEWPORTS];
        let screen = unverifiable.screens[0].clone();
        unverifiable.screens = (0..MAX_SCREENS)
            .map(|index| ScreenInput {
                id: format!("screen-{index}"),
                ..screen.clone()
            })
            .collect();
        assert!(
            validate_plan(&unverifiable).is_err(),
            "a plan with more screenshots than ingest verifies could never pass"
        );
    }

    #[test]
    fn a_complete_passing_report_passes_and_counts_are_recomputed() {
        let (dir, policy) = root();
        let (id, plan_file) = write_plan(&dir, &policy);
        let report = report_for(&dir, &plan_file, &id, "pass", 800);
        std::fs::write(
            dir.join("report.json"),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        let result = ingest(
            &policy,
            &dir.join("plan.json").to_string_lossy(),
            &dir.join("report.json").to_string_lossy(),
        )
        .unwrap();
        assert_eq!(result["verdict"], "pass", "{result}");
        assert_eq!(result["summary"]["pass"], 6);
        assert_eq!(result["declaredSummaryMatches"], true);
        assert_eq!(
            result["figmaReferences"][0]["visualOriginalAt"],
            json!([360])
        );
        assert_eq!(
            result["figmaReferences"][0]["layoutChecksOnlyAt"],
            json!([1200])
        );
        drop(policy);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The screen table is what `devup_screen_plan status` reads: one row per
    /// screen and width, a failed overflow check named as overflow, a failed
    /// navigation pinned on its own route, and a width with no run not-run.
    #[test]
    fn the_screen_table_folds_checks_and_page_errors_per_width() {
        let (dir, policy) = root();
        let (id, plan_file) = write_plan(&dir, &policy);
        let mut report = report_for(&dir, &plan_file, &id, "pass", 800);
        report["runs"][0]["screens"][0]["checks"][0]["status"] = json!("fail");
        report["runs"][0]["screens"][0]["checks"][0]["reason"] = json!("content reaches 412px");
        report["runs"][1]["environmentErrors"] =
            json!([{"kind": "navigation-failed", "message": "/today: HTTP 500"}]);
        std::fs::write(
            dir.join("report.json"),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        let result = ingest(
            &policy,
            &dir.join("plan.json").to_string_lossy(),
            &dir.join("report.json").to_string_lossy(),
        )
        .unwrap();
        let row = |screen: &str, width: u32| {
            result["screenTable"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["screen"] == screen && row["width"] == width)
                .cloned()
                .unwrap()
        };
        assert_eq!(row("today", 360)["status"], "fail");
        assert_eq!(
            row("today", 360)["reasons"][0],
            "horizontal overflow: content reaches 412px"
        );
        assert_eq!(row("today", 1200)["status"], "fail");
        assert_eq!(
            row("today", 1200)["reasons"][0],
            "page error: /today: HTTP 500"
        );
        assert_eq!(row("next", 360)["status"], "pass");
        assert_eq!(row("next", 360)["screenshot"], "360-next.png");

        report["runs"].as_array_mut().unwrap().truncate(1);
        std::fs::write(
            dir.join("report.json"),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        let result = ingest(
            &policy,
            &dir.join("plan.json").to_string_lossy(),
            &dir.join("report.json").to_string_lossy(),
        )
        .unwrap();
        let missing = result["screenTable"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["width"] == 1200)
            .collect::<Vec<_>>();
        assert_eq!(missing.len(), 2);
        assert!(missing.iter().all(|row| row["status"] == "not-run"));
        drop(policy);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Only the screen's drawn width has a visual original, and its verified
    /// screenshot comes back as a ready comparison: what the audit knows is
    /// filled in, what only the consumer knows is named.
    #[test]
    fn a_drawn_width_screenshot_comes_back_as_a_ready_visual_comparison() {
        let (dir, policy) = root();
        let (id, plan_file) = write_plan(&dir, &policy);
        let mut report = report_for(&dir, &plan_file, &id, "pass", 800);
        report["executor"] = json!({"browser": {"name": "chromium", "version": "131.0.1"}});
        report["environment"] = json!({"os": "linux 6.1", "locale": null, "timezone": "UTC"});
        std::fs::write(
            dir.join("report.json"),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        let result = ingest(
            &policy,
            &dir.join("plan.json").to_string_lossy(),
            &dir.join("report.json").to_string_lossy(),
        )
        .unwrap();
        let comparisons = result["visualComparisons"].as_array().unwrap();
        assert_eq!(
            comparisons.len(),
            1,
            "only today at 360 was drawn: {result}"
        );
        let comparison = &comparisons[0];
        assert_eq!(comparison["screen"], "today");
        assert_eq!(comparison["nodeId"], "1:138");
        let arguments = &comparison["call"]["arguments"];
        assert_eq!(comparison["call"]["tool"], "devup_visual_compare");
        assert!(
            arguments["actual"]
                .as_str()
                .unwrap()
                .ends_with("360-today.png")
        );
        let environment = &arguments["environment"];
        assert_eq!(environment["actualPng"], arguments["actual"]);
        assert_eq!(environment["environmentStatus"], "valid");
        assert_eq!(
            environment["renderer"],
            json!({"name": "playwright-chromium", "version": "131.0.1"})
        );
        assert_eq!(environment["timezone"], "UTC");
        let must_add = comparison["consumerMustAdd"].as_array().unwrap();
        assert!(must_add.contains(&json!("environment.locale")));
        assert!(must_add.contains(&json!("environment.tsxResource")));
        assert_eq!(comparison["blockers"], json!([]), "{comparison}");
        drop(policy);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Every failed check comes back as a finding of one class with the
    /// Figma node, the code file and a fix hint; a capture comes back as a
    /// ready asset comparison against the node's own render, and checks
    /// limited to a width are neither planned nor not-run elsewhere.
    #[test]
    fn failures_are_classified_and_captures_become_asset_comparisons() {
        let (dir, policy) = root();
        let node = |id: &str| json!({"css": format!("[data-figma-node-id=\"{id}\"]")});
        let input: PlanInput = serde_json::from_value(json!({
            "baseUrl": "http://127.0.0.1:4173", "allowedOrigins": ["http://127.0.0.1:4173"],
            "viewports": [{"width": 1440, "height": 900}, {"width": 390, "height": 900}],
            "screens": [{"id": "home", "routePath": "/", "componentPath": "src/app/page.tsx",
                "figma": {"nodeId": "1:1", "drawnWidths": [1440], "frames": [{"width": 1440, "nodeId": "1:1", "artifactId": "art-1"}]},
                "checks": [
                    {"kind": "page-health"},
                    {"kind": "bounds", "subject": node("1:3"), "container": node("1:2"), "nodeId": "1:3",
                        "expected": {"x": 0, "y": 0, "width": 10, "height": 10}, "widths": [1440]},
                    {"kind": "interaction", "state": "hover", "widths": [1440]},
                    {"kind": "capture", "subject": node("1:4"), "nodeId": "1:4", "widths": [1440],
                        "reference": {"scope": "asset", "artifactId": "art-1", "assetId": "icon"}},
                ]}],
        }))
        .unwrap();
        let written = plan(
            input,
            &policy,
            Some(&dir.join("plan.json").to_string_lossy()),
        )
        .unwrap();
        assert_eq!(
            written["expected"]["checkItems"], 5,
            "four at 1440, health alone at 390"
        );
        let plan_bytes = std::fs::read(dir.join("plan.json")).unwrap();
        let page = png(1440, 900);
        let icon = png(24, 24);
        std::fs::write(dir.join("page.png"), &page).unwrap();
        std::fs::write(dir.join("icon.png"), &icon).unwrap();
        let shot = |bytes: &[u8], path: &str| json!({"path": path, "sha256": sha256(bytes), "bytes": bytes.len()});
        let report = json!({"schemaVersion": 1, "kind": REPORT_KIND, "planId": written["planId"],
        "planSha256": sha256(&plan_bytes), "status": "completed", "runs": [
            {"viewport": {"width": 1440, "height": 900, "deviceScaleFactor": 1}, "environmentErrors": [],
             "screens": [{"id": "home", "status": "completed", "screenshot": shot(&page, "page.png"), "checks": [
                {"index": 0, "status": "pass", "reason": "ok", "measured": {}},
                {"index": 1, "status": "fail", "reason": "position: y off by 60px", "measured": {}},
                {"index": 2, "status": "fail", "reason": "hover changes nothing on 1 of 2", "measured": {}},
                {"index": 3, "status": "pass", "reason": "captured", "measured": {}, "screenshot": shot(&icon, "icon.png")},
             ]}]},
            {"viewport": {"width": 390, "height": 900, "deviceScaleFactor": 1}, "environmentErrors": [],
             "screens": [{"id": "home", "status": "completed", "checks": [
                {"index": 0, "status": "fail", "reason": "the page shows \"Application error\"", "measured": {}},
             ]}]},
        ]});
        std::fs::write(
            dir.join("report.json"),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        let result = ingest(
            &policy,
            &dir.join("plan.json").to_string_lossy(),
            &dir.join("report.json").to_string_lossy(),
        )
        .unwrap();
        assert_eq!(result["summary"]["notRun"], 0, "{result}");
        let findings = result["findings"].as_array().unwrap();
        let classes = findings
            .iter()
            .map(|finding| {
                (
                    finding["class"].as_str().unwrap(),
                    finding["width"].as_u64().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            classes,
            [
                ("layout-position", 1440),
                ("interaction", 1440),
                ("runtime-error", 390)
            ]
        );
        let position = &findings[0];
        assert_eq!(position["nodeId"], "1:3");
        assert_eq!(position["codeFile"], "src/app/page.tsx");
        assert_eq!(position["id"], "home|bounds|1:3|1440");
        assert!(
            position["fixHint"]
                .as_str()
                .unwrap()
                .contains("Layout is exact")
        );
        assert!(
            findings[2]["fixHint"]
                .as_str()
                .unwrap()
                .contains("error-boundary")
        );

        let comparisons = result["visualComparisons"].as_array().unwrap();
        let page_call = comparisons
            .iter()
            .find(|c| c["class"] == "page-visual")
            .unwrap();
        assert_eq!(
            page_call["call"]["arguments"]["reference"],
            json!({"artifactId": "art-1"})
        );
        assert_eq!(page_call["call"]["arguments"]["threshold"], PAGE_THRESHOLD);
        let asset_call = comparisons
            .iter()
            .find(|c| c["class"] == "asset-mismatch")
            .unwrap();
        assert_eq!(asset_call["id"], "home|asset|1:4|1440");
        assert_eq!(
            asset_call["call"]["arguments"]["reference"],
            json!({"artifactId": "art-1", "assetId": "icon"})
        );
        assert_eq!(
            asset_call["call"]["arguments"]["threshold"],
            ASSET_THRESHOLD
        );
        assert!(
            asset_call["call"]["arguments"]["diffPath"]
                .as_str()
                .unwrap()
                .ends_with("home_asset_1_4_1440.png")
        );
        assert!(
            asset_call["ifItFails"]["fixHint"]
                .as_str()
                .unwrap()
                .contains("multi-colour SVG")
        );
        let wide = result["screenTable"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["width"] == 1440)
            .unwrap();
        assert_eq!(wide["columns"]["geometry"], "fail");
        assert_eq!(wide["columns"]["asset"], "1 to compare");
        assert_eq!(wide["columns"]["page"], "1 to compare");
        drop(policy);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failure_fails_missing_items_are_inconclusive_and_edits_are_rejected() {
        let (dir, policy) = root();
        let (id, plan_file) = write_plan(&dir, &policy);
        let plan_path = dir.join("plan.json").to_string_lossy().into_owned();
        let report_path = dir.join("report.json").to_string_lossy().into_owned();

        let failing = report_for(&dir, &plan_file, &id, "fail", 800);
        std::fs::write(&report_path, serde_json::to_vec(&failing).unwrap()).unwrap();
        let result = ingest(&policy, &plan_path, &report_path).unwrap();
        assert_eq!(result["verdict"], "fail");
        assert_eq!(
            result["declaredSummaryMatches"], false,
            "the declared summary claimed passes"
        );

        let mut partial = report_for(&dir, &plan_file, &id, "pass", 800);
        partial["runs"].as_array_mut().unwrap().pop();
        std::fs::write(&report_path, serde_json::to_vec(&partial).unwrap()).unwrap();
        let result = ingest(&policy, &plan_path, &report_path).unwrap();
        assert_eq!(result["verdict"], "inconclusive");
        assert_eq!(result["summary"]["notRun"], 3);

        let mut environment = report_for(&dir, &plan_file, &id, "fail", 800);
        environment["runs"][0]["environmentErrors"] =
            json!([{"kind": "font-missing", "message": "Pretendard"}]);
        std::fs::write(&report_path, serde_json::to_vec(&environment).unwrap()).unwrap();
        assert_eq!(
            ingest(&policy, &plan_path, &report_path).unwrap()["verdict"],
            "inconclusive"
        );

        let cropped = report_for(&dir, &plan_file, &id, "pass", 700);
        std::fs::write(&report_path, serde_json::to_vec(&cropped).unwrap()).unwrap();
        let result = ingest(&policy, &plan_path, &report_path).unwrap();
        assert_eq!(
            result["verdict"], "inconclusive",
            "a shorter capture than the viewport: {result}"
        );

        let tampered = report_for(&dir, &plan_file, &id, "pass", 800);
        std::fs::write(&report_path, serde_json::to_vec(&tampered).unwrap()).unwrap();
        std::fs::write(dir.join("360-today.png"), png(360, 741)).unwrap();
        assert_eq!(
            ingest(&policy, &plan_path, &report_path).unwrap()["reason"],
            "artifact-changed"
        );

        let mut edited: Value = serde_json::from_slice(&plan_file).unwrap();
        edited["plan"]["viewports"][0]["width"] = json!(390);
        std::fs::write(&plan_path, serde_json::to_vec(&edited).unwrap()).unwrap();
        assert_eq!(
            ingest(&policy, &plan_path, &report_path).unwrap()["reason"],
            "plan-edited"
        );
        drop(policy);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn screenshot_paths_cannot_leave_the_report_directory() {
        let (dir, policy) = root();
        let viewport = ViewportInput {
            width: 360,
            height: 740,
            device_scale_factor: 1.0,
            mobile: false,
            touch: false,
        };
        for path in ["../outside.png", "/etc/passwd", "C:\\Windows\\win.ini"] {
            let shot = json!({"path": path, "sha256": "0", "bytes": 1});
            assert!(matches!(
                verify_screenshot(&policy, &dir, &shot, &viewport),
                Err(Integrity::Unverified(_))
            ));
        }
        drop(policy);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
