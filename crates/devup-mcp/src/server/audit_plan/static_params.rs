

/// `generateStaticParams` in the route's page: literal params in its body,
/// else the param's values in a local data file it reads.
pub(super) fn static_params(
    root: &Path,
    app_dir: &str,
    page_file: &str,
    pattern: &str,
) -> Vec<String> {
    let page = root.join(page_file);
    let Ok(source) = std::fs::read_to_string(&page) else {
        return Vec::new();
    };
    let Some(at) = source.find("generateStaticParams") else {
        return Vec::new();
    };
    let body = &source[at..];
    let body = body[1..]
        .find("\nexport ")
        .map_or(body, |end| &body[..=end]);
    let names = params(pattern);
    let collect = |text: &str| {
        let lists = names
            .iter()
            .map(|(name, _)| values_of(text, name))
            .collect::<Vec<_>>();
        let count = lists.iter().map(Vec::len).min().unwrap_or(0);
        (0..count)
            .filter_map(|index| {
                let values = names
                    .iter()
                    .zip(&lists)
                    .map(|((name, _), list)| (name.clone(), list[index].clone()))
                    .collect::<BTreeMap<_, _>>();
                fill(pattern, &values)
            })
            .collect::<Vec<_>>()
    };
    let literal = collect(body);
    if !literal.is_empty() {
        return literal;
    }
    let page_dir = page.parent().unwrap_or(root);
    for (imported, module) in local_imports(&source) {
        let used = imported
            .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
            .filter(|name| !name.is_empty() && *name != "as")
            .any(|name| body.contains(name));
        if !used {
            continue;
        }
        let text = match module.strip_prefix("@/") {
            Some(rest) => read_module(&root.join(app_dir).join("src"), rest)
                .or_else(|| read_module(&root.join(app_dir), rest)),
            None => read_module(page_dir, &module),
        };
        if let Some(text) = text {
            let from_data = collect(&text);
            if !from_data.is_empty() {
                return from_data;
            }
        }
    }
    Vec::new()
}

/// The first text of a frame, depth first: a detail page's title.
fn first_text(sources: &Sources, frame: &str) -> Option<String> {
    let (_, payload) = sources.frame(frame)?;
    let mut stack = vec![frame.to_owned()];
    while let Some(id) = stack.pop() {
        let node = payload.snapshot.nodes.get(&id)?;
        let view = node.typed_view();
        if let Some(text) = view
            .string("characters")
            .filter(|text| !text.trim().is_empty())
        {
            return Some(text.to_owned());
        }
        let mut children = view.child_ids().map(str::to_owned).collect::<Vec<_>>();
        children.reverse();
        stack.extend(children);
    }
    None
}

fn sample_segment(text: &str) -> String {
    let words = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(6)
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    if words.is_empty() {
        "sample".to_owned()
    } else {
        words.join("-")
    }
}

/// A dynamic route's concrete URLs: its `generateStaticParams`, else the
/// links the audited pages hold into it, else a sample from its Figma detail
/// frame (an assumption). At most ten.
pub(super) fn dynamic_urls(root: &str, route: &Value, links: &Value, sources: &Sources) -> Dynamic {
    let pattern = route["url"].as_str().unwrap_or_default();
    let app_dir = route["app"]
        .as_str()
        .filter(|app| !app.is_empty())
        .unwrap_or(".");
    let cap = |mut urls: Vec<String>| {
        urls.sort();
        urls.dedup();
        urls.truncate(MAX_DYNAMIC_URLS);
        urls
    };
    if let Some(file) = route["file"].as_str() {
        let found = static_params(Path::new(root), app_dir, file, pattern);
        if !found.is_empty() {
            return Dynamic {
                urls: cap(found),
                source: "generateStaticParams",
                why: String::new(),
                assumption: None,
            };
        }
    }
    let linked = links
        .as_object()
        .into_iter()
        .flatten()
        .flat_map(|(_, hrefs)| hrefs.as_array().cloned().unwrap_or_default())
        .filter_map(|href| href.as_str().map(str::to_owned))
        .filter(|href| super::navigation::matches_route(pattern, href))
        .collect::<Vec<_>>();
    if !linked.is_empty() {
        return Dynamic {
            urls: cap(linked),
            source: "links on audited pages",
            why: String::new(),
            assumption: None,
        };
    }
    let frame = route["nodeIds"][0].as_str().unwrap_or_default();
    if let Some(text) = first_text(sources, frame) {
        let segment = sample_segment(&text);
        let values = params(pattern)
            .into_iter()
            .map(|(name, _)| (name, segment.clone()))
            .collect::<BTreeMap<_, _>>();
        if let Some(url) = fill(pattern, &values) {
            return Dynamic {
                assumption: Some(format!(
                    "{pattern} audited at {url}: the sample comes from the detail frame {frame}'s title \"{text}\"; the app must serve it."
                )),
                urls: vec![url],
                source: "Figma detail frame",
                why: String::new(),
            };
        }
    }
    Dynamic {
        urls: Vec::new(),
        source: "none",
        why: "No generateStaticParams with literal or local-data params, no link to it on an audited page (run the audit once and auditPlan again), and no detail frame text to sample.".to_owned(),
        assumption: None,
    }
}

const STRIP_PLUGIN: &[u8] =
    include_bytes!("../../../../../harness/runtime-audit/strip-figma-node-ids.mjs");
const STRIP_FILE: &str = "vite-plugin-strip-figma-node-ids.mjs";

/// Puts the plugin that strips the audit attributes from production builds
/// next to each app's vite.config, once, and says whether the config uses
/// it yet.
fn production_strip(
    routes: &[&Value],
    root: &str,
    policy: &OutputPolicy,
) -> Result<Vec<Value>, DevupError> {
    let apps = routes
        .iter()
        .map(|route| {
            route["app"]
                .as_str()
                .filter(|app| !app.is_empty())
                .unwrap_or(".")
                .to_owned()
        })
        .collect::<BTreeSet<_>>();
    let mut out = Vec::new();
    for app in apps {
        let dir = Path::new(root).join(&app);
        let file = dir.join(STRIP_FILE);
        let written = if file.exists() {
            false
        } else {
            let target = policy.resolve(&file.to_string_lossy())?;
            let mut transaction = super::output::OutputTransaction::new();
            transaction.stage("stripPlugin", target, STRIP_PLUGIN)?;
            transaction.commit()?;
            true
        };
        let config = [
            "vite.config.ts",
            "vite.config.mts",
            "vite.config.js",
            "vite.config.mjs",
        ]
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file());
        let wired = config
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .is_some_and(|text| text.contains(STRIP_FILE));
        out.push(json!({
            "app": app,
            "plugin": file.display().to_string(),
            "written": written,
            "viteConfig": config.map(|path| path.display().to_string()),
            "wired": wired,
            "add": format!("import stripFigmaNodeIds from './{STRIP_FILE}'  // then plugins: [stripFigmaNodeIds(), vinext()]"),
            "nextCli": "On the next CLI instead: compiler.reactRemoveProperties: process.env.DEVUP_AUDIT === '1' ? false : { properties: ['^data-figma-node-ids?$'] }",
            "audit": "Audit against `bun run dev` or an audit build: DEVUP_AUDIT=1 bun run build.",
            "verify": "After `bun run build`, `grep -rl data-figma-node-id dist` (or the app's build output) must find nothing.",
        }));
    }
    Ok(out)
}

/// One devup_visual_compare result for a comparison status returned.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct VisualResultInput {
    /// The visualComparisons id.
    pub id: String,
    /// The comparison's verdict.
    #[schemars(extend("enum" = ["pass", "fail", "inconclusive"]))]
    pub verdict: String,
    /// visual.changedRatio from the result.
    #[serde(default)]
    pub changed_ratio: Option<f64>,
    /// The diffPath the result wrote.
    #[serde(default)]
    pub diff_path: Option<String>,
    /// Why it was inconclusive, when it was.
    #[serde(default)]
    pub reason: Option<String>,
}

/// The order a URL's failures are worked in: a page that does not render
/// first, then overflow, then layout, then pixels, then states.
fn class_rank(class: &str) -> usize {
    [
        "runtime-error",
        "overflow",
        "layout-position",
        "layout-size",
        "asset-mismatch",
        "section-visual",
        "page-visual",
        "interaction",
        "view-transition",
    ]
    .iter()
    .position(|candidate| *candidate == class)
    .unwrap_or(9)
}

/// Folds one audit run (and the visual results for it) into the plan's
/// finding records. A finding counts a failed run once per new evidence -
/// a new report for a check, a new screenshot for a comparison - so calling
/// status twice on one report does not burn an attempt. After
/// `max_fix_attempts` fix-and-rerun rounds that still fail it is
/// unresolved, with its evidence, and leaves the work queue.
pub(super) fn record_round(
    plan: &mut Value,
    audit: &Value,
    report_sha: &str,
    visual: &[VisualResultInput],
    node_files: &BTreeMap<String, (String, String)>,
    max_fix_attempts: u32,
) -> Value {
    let audited = audit["screenTable"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some((
                row["routePath"].as_str()?.to_owned(),
                row["width"].as_u64()?,
            ))
        })
        .collect::<BTreeSet<_>>();
    let comparisons = audit["visualComparisons"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut current = Vec::<(Value, String)>::new();
    for finding in audit["findings"].as_array().into_iter().flatten() {
        current.push((finding.clone(), report_sha.to_owned()));
    }
    let mut compared = BTreeSet::new();
    let mut inconclusive = Vec::new();
    for result in visual {
        let Some(comparison) = comparisons.iter().find(|c| c["id"] == result.id.as_str()) else {
            inconclusive.push(
                json!({"id": result.id, "why": "no comparison with this id in this audit run"}),
            );
            continue;
        };
        compared.insert(result.id.clone());
        match result.verdict.as_str() {
            "fail" => {
                let threshold = comparison["call"]["arguments"]["threshold"].as_f64();
                let reason = match (result.changed_ratio, threshold) {
                    (Some(ratio), Some(threshold)) => format!(
                        "{:.1}% of pixels differ from the Figma render (allowed {:.1}%)",
                        ratio * 100.0,
                        threshold * 100.0
                    ),
                    _ => "differs from the Figma render".to_owned(),
                };
                current.push((
                    json!({
                        "id": result.id,
                        "class": comparison["class"],
                        "screen": comparison["screen"],
                        "routePath": comparison["routePath"],
                        "width": comparison["viewport"]["width"],
                        "kind": "visual",
                        "label": comparison["label"],
                        "nodeId": comparison["nodeId"],
                        "codeFile": comparison["codeFile"],
                        "reason": reason,
                        "fixHint": comparison["ifItFails"]["fixHint"],
                        "evidence": {"screenshot": comparison["screenshot"]["path"], "diff": result.diff_path,
                            "reference": comparison["call"]["arguments"]["reference"]},
                    }),
                    comparison["screenshot"]["sha256"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                ));
            }
            "inconclusive" => inconclusive.push(json!({"id": result.id, "class": comparison["class"],
                "why": result.reason.clone().unwrap_or_else(|| "the comparison was inconclusive".to_owned())})),
            _ => {}
        }
    }
    let pending = comparisons
        .iter()
        .filter(|comparison| {
            !compared.contains(comparison["id"].as_str().unwrap_or_default())
                && comparison["blockers"].as_array().is_none_or(Vec::is_empty)
        })
        .map(|comparison| comparison["id"].clone())
        .collect::<Vec<_>>();
    // A converter bug is reported once and kept, whatever later runs show.
    for (finding, _) in &current {
        if finding["class"] == "converter-bug"
            && let Some(node) = finding["nodeId"].as_str()
        {
            plan["audit"]["converterBugs"][node] = json!({
                "nodeId": node, "routePath": finding["routePath"], "width": finding["width"],
                "reason": finding["reason"], "repro": finding["evidence"]["repro"],
                "codeFile": finding["codeFile"],
            });
        }
    }
    let records = &mut plan["audit"]["findings"];
    if !records.is_object() {
        *records = json!({});
    }
    let records = records.as_object_mut().expect("an object");
    let failing = current
        .iter()
        .filter_map(|(finding, _)| finding["id"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    for (mut finding, key) in current {
        let id = finding["id"].as_str().unwrap_or_default().to_owned();
        if let Some((_, file)) = finding["nodeId"]
            .as_str()
            .and_then(|node| node_files.get(node))
        {
            finding["codeFile"] = json!(file);
        }
        let record = records
            .entry(id)
            .or_insert_with(|| json!({"failedRuns": 0}));
        if record["lastEvidence"] != key.as_str() {
            record["failedRuns"] = json!(record["failedRuns"].as_u64().unwrap_or(0) + 1);
            record["lastEvidence"] = json!(key);
        }
        let fix_attempts = record["failedRuns"].as_u64().unwrap_or(1).saturating_sub(1);
        for field in [
            "class",
            "screen",
            "routePath",
            "width",
            "kind",
            "label",
            "nodeId",
            "codeFile",
            "reason",
            "fixHint",
            "evidence",
        ] {
            record[field] = finding[field].clone();
        }
        record["fixAttempts"] = json!(fix_attempts);
        // A deterministic result does not change by retrying: a converter
        // bug leaves the fix loop at once and never spends attempts.
        record["status"] = json!(if finding["class"] == "converter-bug" {
            "converter-bug"
        } else if fix_attempts >= u64::from(max_fix_attempts) {
            "unresolved"
        } else {
            "open"
        });
    }
    // Anything this run measured again and did not see fail is fixed. A
    // comparison is fixed only by a passing result for it.
    let passed = visual
        .iter()
        .filter(|result| result.verdict == "pass")
        .map(|result| result.id.as_str())
        .collect::<BTreeSet<_>>();
    for (id, record) in records.iter_mut() {
        if failing.contains(id) || record["status"] == "resolved" {
            continue;
        }
        let measured_again = if record["kind"] == "visual" {
            passed.contains(id.as_str())
        } else {
            record["routePath"]
                .as_str()
                .zip(record["width"].as_u64())
                .is_some_and(|(route, width)| audited.contains(&(route.to_owned(), width)))
        };
        if measured_again {
            record["status"] = json!("resolved");
        }
    }
    json!({"pendingComparisons": pending, "inconclusiveComparisons": inconclusive})
}

/// The open and unresolved findings in work order, most basic first.
pub(super) fn findings_by_status(plan: &Value, status: &str) -> Vec<Value> {
    let mut list = plan["audit"]["findings"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, record)| record["status"] == status)
        .map(|(id, record)| {
            let mut record = record.clone();
            record["id"] = json!(id);
            record
        })
        .collect::<Vec<_>>();
    list.sort_by_key(|record| {
        (
            record["routePath"].as_str().unwrap_or_default().to_owned(),
            class_rank(record["class"].as_str().unwrap_or_default()),
            std::cmp::Reverse(record["width"].as_u64().unwrap_or(0)),
        )
    });
    list
}

/// Per URL: what to do next, blocked items last. Nothing here waits on
/// the user; a blocked item waits on a pending decision taken at the end.
pub(super) fn work_queue(
    plan: &Value,
    rows: &[Value],
    pending_comparisons: &[Value],
) -> Vec<Value> {
    let open = findings_by_status(plan, "open");
    let decisions = plan["pendingDecisions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let audited_urls = plan["audit"]["plans"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["url"].as_str())
        .collect::<BTreeSet<_>>();
    rows.iter()
        .map(|row| {
            let url = row["url"].as_str().unwrap_or_default();
            let screen = audit_screen(plan, url);
            let mut items = Vec::new();
            let mut blocked = Vec::new();
            for decision in &decisions {
                if decision["status"] == "open"
                    && decision["about"].as_str().is_some_and(|about| about.contains(url))
                {
                    blocked.push(json!({"do": "blocked", "on": decision["id"],
                        "why": decision["question"], "meanwhile": "work the rest; it is taken with assume at the end"}));
                }
            }
            if row["implemented"] != "yes" {
                items.push(json!({"do": "build", "file": row["file"]}));
            }
            if let Some(reason) = plan["audit"]["inconclusive"][url].as_str() {
                items.push(json!({"do": "audit inconclusive", "why": reason,
                    "how": "install the runner when it can be; the rest of the work goes on"}));
            } else if !audited_urls.contains(url) {
                items.push(json!({"do": "audit", "how": "devup_screen_plan action auditPlan, run the plan, then status"}));
            } else if row["widths"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|width| width["result"] == "not-run")
            {
                items.push(json!({"do": "run the audit", "why": "some widths have no audit result yet"}));
            }
            for finding in open.iter().filter(|finding| finding["routePath"] == url) {
                items.push(json!({"do": "fix", "finding": finding["id"], "class": finding["class"],
                    "width": finding["width"], "codeFile": finding["codeFile"], "fixHint": finding["fixHint"],
                    "attempt": finding["fixAttempts"].as_u64().unwrap_or(0) + 1}));
            }
            let comparisons = pending_comparisons
                .iter()
                .filter_map(Value::as_str)
                .filter(|id| screen.as_deref().is_some_and(|screen| id.starts_with(&format!("{screen}|"))))
                .count();
            if comparisons > 0 {
                items.push(json!({"do": "compare", "count": comparisons,
                    "how": "run each visualComparisons call and pass the results back as visualResults"}));
            }
            let done = items.is_empty();
            items.extend(blocked);
            let reported = plan["audit"]["converterBugs"]
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(_, bug)| bug["routePath"] == url)
                .map(|(_, bug)| json!({"converterBug": bug["nodeId"], "repro": bug["repro"],
                    "note": "reported, not retried; a local override needs a comment naming the repro"}))
                .collect::<Vec<_>>();
            json!({"url": url, "items": items, "reported": reported, "done": done})
        })
        .collect()
}

fn audit_screen(plan: &Value, url: &str) -> Option<String> {
    plan["audit"]["plans"]
        .as_array()?
        .iter()
        .find(|entry| entry["url"] == url)?["screen"]
        .as_str()
        .map(str::to_owned)
}

/// The report the job ends with: every URL at every width by health,
/// geometry, the three visual levels and interaction, with screenshots and
/// diffs, then the unresolved findings with their evidence and the pending
/// decisions taken. The plan's own lists (assumptions, design fixes,
/// exclusions, defaults) and the dependency line follow.
pub(super) fn final_report(plan: &Value, rows: &[Value], trailing: &str) -> String {
    let cell = |value: &Value| match value {
        Value::Null => "-".to_owned(),
        Value::String(text) => text.replace('|', "\\|"),
        other => other.to_string(),
    };
    let mut out = String::from(
        "| URL | width | health | geometry | drift / converter bugs | page | section | asset | interaction | screenshot | diffs |\n|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    let records = plan["audit"]["findings"].as_object();
    for row in rows {
        let url = row["url"].as_str().unwrap_or_default();
        for width in row["widths"].as_array().into_iter().flatten() {
            let columns = &width["columns"];
            let diffs = records
                .into_iter()
                .flatten()
                .filter(|(_, record)| {
                    record["routePath"] == url
                        && record["width"] == width["width"]
                        && record["status"] != "resolved"
                })
                .filter_map(|(_, record)| record["evidence"]["diff"].as_str())
                .collect::<Vec<_>>();
            let count = |class: &str| {
                records
                    .into_iter()
                    .flatten()
                    .filter(|(_, record)| {
                        record["routePath"] == url
                            && record["width"] == width["width"]
                            && record["class"] == class
                            && record["status"] != "resolved"
                    })
                    .count()
            };
            out.push_str(&format!(
                "| {url} | {} | {} | {} | {} / {} | {} | {} | {} | {} | {} | {} |\n",
                width["width"],
                cell(&columns["health"]),
                cell(&columns["geometry"]),
                count("code-drift"),
                count("converter-bug"),
                cell(&columns["page"]),
                cell(&columns["section"]),
                cell(&columns["asset"]),
                cell(&columns["interaction"]),
                width["screenshots"]
                    .as_array()
                    .and_then(|shots| shots.first())
                    .map_or("-".to_owned(), cell),
                if diffs.is_empty() {
                    "-".to_owned()
                } else {
                    diffs.join(", ")
                },
            ));
        }
    }
    let unresolved = findings_by_status(plan, "unresolved");
    out.push_str("\nUnresolved findings (after the fix-attempt budget):\n");
    if unresolved.is_empty() {
        out.push_str("- none\n");
    }
    for finding in &unresolved {
        out.push_str(&format!(
            "- {} at {}px, {} {}: {} (code: {}; screenshot: {}; diff: {})\n",
            cell(&finding["routePath"]),
            finding["width"],
            cell(&finding["class"]),
            cell(&finding["nodeId"]),
            cell(&finding["reason"]),
            cell(&finding["codeFile"]),
            cell(&finding["evidence"]["screenshot"]),
            cell(&finding["evidence"]["diff"]),
        ));
    }
    out.push_str("\nConverter bugs (filed, not retried):\n");
    let bugs = plan["audit"]["converterBugs"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    if bugs.is_empty() {
        out.push_str("- none\n");
    }
    for bug in bugs.values() {
        out.push_str(&format!(
            "- {} at {}px, {}: {} (repro: {}; code: {})\n",
            cell(&bug["routePath"]),
            bug["width"],
            cell(&bug["nodeId"]),
            cell(&bug["reason"]),
            cell(&bug["repro"]),
            cell(&bug["codeFile"]),
        ));
    }
    out.push_str("\nPending decisions:\n");
    let decisions = plan["pendingDecisions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if decisions.is_empty() {
        out.push_str("- none\n");
    }
    for decision in &decisions {
        out.push_str(&format!(
            "- {}: {} -> {} ({})\n",
            cell(&decision["id"]),
            cell(&decision["question"]),
            cell(decision.get("taken").unwrap_or(&decision["recommended"])),
            match decision["status"].as_str() {
                Some("taken") => "taken at the end",
                Some("left-unbuilt") => "destructive: left unbuilt",
                _ => "still open",
            }
        ));
    }
    out.push_str(trailing);
    let dependencies = &plan["dependencies"];
    out.push_str(&if dependencies.is_object() {
        format!(
            "\nDependencies: `{}` bumped {} packages; pins: {}.\n",
            cell(&dependencies["command"]),
            dependencies["bumped"],
            dependencies["pins"]
                .as_array()
                .filter(|pins| !pins.is_empty())
                .map_or("none".to_owned(), |pins| pins
                    .iter()
                    .map(|pin| format!(
                        "{}@{} ({})",
                        cell(&pin["package"]),
                        cell(&pin["version"]),
                        cell(&pin["reason"])
                    ))
                    .collect::<Vec<_>>()
                    .join(", "))
        )
    } else {
        "\nDependencies: the update step has not been recorded.\n".to_owned()
    });
    out
}

#[cfg(test)]
#[path = "../audit_plan_tests.rs"]
mod tests;

/// Geometry is exact, so a box that fails at a drawn width is classified by
/// cause: the code drifted from what the export wrote, or it did not and the
/// converter is wrong. A converter bug gets its repro written under
/// `.devup/converter-bugs/`.
pub(super) fn classify_geometry(
    audit: &mut Value,
    project_root: &str,
    sources: &[String],
    policy: &OutputPolicy,
) {
    let Some(findings) = audit["findings"].as_array_mut() else {
        return;
    };
    for finding in findings {
        if !matches!(finding["kind"].as_str(), Some("bounds" | "dimensions")) {
            continue;
        }
        let Some(node) = finding["nodeId"].as_str().map(str::to_owned) else {
            continue;
        };
        match super::export_record::geometry_cause(Path::new(project_root), &node, sources) {
            super::export_record::Cause::Drift {
                file,
                missing,
                added,
            } => {
                finding["class"] = json!("code-drift");
                finding["codeFile"] = json!(file);
                finding["evidence"]["drift"] = json!({"missing": missing, "added": added});
                finding["fixHint"] = json!(format!(
                    "{} Missing or changed: {}. Added layout props: {}.",
                    runtime_audit::fix_hint("code-drift", "bounds", &Value::Null),
                    if missing.is_empty() {
                        "none".to_owned()
                    } else {
                        missing.join(", ")
                    },
                    if added.is_empty() {
                        "none".to_owned()
                    } else {
                        added.join(", ")
                    },
                ));
            }
            super::export_record::Cause::ConverterBug {
                file,
                record,
                exported,
            } => {
                let width = finding["width"].clone();
                match super::export_record::write_repro(
                    policy,
                    project_root,
                    &node,
                    &width,
                    finding,
                    &file,
                    &record,
                    &exported,
                ) {
                    Ok((path, _)) => {
                        finding["class"] = json!("converter-bug");
                        finding["codeFile"] = json!(file);
                        finding["evidence"]["repro"] = json!(path);
                        finding["fixHint"] = json!(runtime_audit::fix_hint(
                            "converter-bug",
                            "bounds",
                            &Value::Null
                        ));
                    }
                    Err(error) => {
                        finding["evidence"]["reproError"] = json!(error.message);
                    }
                }
            }
            super::export_record::Cause::Unknown(why) => {
                finding["evidence"]["cause"] = json!(why);
            }
        }
    }
}
