fn gate_source_anchors(built: &mut UrlPlan, app: &Path) {
    // Consumer-declared mappings are evidence; DOM order or component names
    // alone cannot establish which Figma node a rendered element represents.
    let mapping_path = app.join(".devup/rendered-targets.json");
    let mappings = std::fs::read(&mapping_path).ok().and_then(|bytes| {
        if bytes.len() > 1024 * 1024 {
            return None;
        }
        serde_json::from_slice::<Value>(&bytes).ok()
    });
    let route = built.plan.screens[0]
        .route_path
        .as_deref()
        .unwrap_or_default();
    let selectors: BTreeMap<String, String> = mappings
        .as_ref()
        .and_then(|document| document["routes"][route]["nodes"].as_object())
        .into_iter()
        .flatten()
        .filter_map(|(id, value)| {
            let css = value.as_str()?;
            (!id.is_empty()
                && id.len() <= 300
                && !css.trim().is_empty()
                && css.chars().count() <= 300)
                .then(|| (id.clone(), css.to_owned()))
        })
        .collect();
    if !selectors.is_empty() {
        built.notes.push(format!("Explicit rendered targets from {} replace node-attribute selectors; missing or ambiguous DOM matches remain inconclusive.", mapping_path.display()));
    }
    fn scan(path: &Path, text: &mut String) {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
                continue;
            }
            if path.is_dir() {
                scan(&path, text);
            } else if matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("tsx" | "jsx" | "ts" | "js")
            ) && let Ok(source) = std::fs::read_to_string(path)
            {
                text.push_str(&source);
                text.push('\n');
            }
        }
    }
    let mut source = String::new();
    scan(&app.join("src"), &mut source);
    if !app.join("src").exists() {
        scan(&app.join("app"), &mut source);
        scan(&app.join("components"), &mut source);
    }
    let mut anchors = BTreeSet::new();
    for tail in source.split("data-figma-node-id").skip(1) {
        let (plural, tail) = tail
            .strip_prefix('s')
            .map_or((false, tail), |tail| (true, tail));
        let Some(tail) = tail.trim_start().strip_prefix('=') else {
            continue;
        };
        let tail = tail.trim_start();
        let Some(quote @ ('\'' | '"')) = tail.chars().next() else {
            continue;
        };
        if let Some(value) = tail[1..].split(quote).next() {
            for value in value.split_whitespace() {
                let id = if plural {
                    value.split_once(':').map_or(value, |(_, id)| id)
                } else {
                    value
                };
                anchors.insert(id.to_owned());
            }
        }
    }
    for check in &mut built.plan.screens[0].checks {
        for locator in [&mut check.subject, &mut check.container]
            .into_iter()
            .flatten()
        {
            if let Some(css) = locator.css.as_deref()
                && css.contains("data-figma-node-id")
                && let Some(id) = css.split('"').nth(1)
                && let Some(mapped) = selectors.get(id)
            {
                locator.css = Some(mapped.clone());
            }
        }
        if let Some(complete) = &mut check.completeness {
            for id in complete.sections.iter().chain(&complete.images) {
                if let Some(css) = selectors.get(id) {
                    complete.node_selectors.insert(id.clone(), css.clone());
                }
            }
        }
        let anchored = [&check.subject, &check.container]
            .into_iter()
            .flatten()
            .filter_map(|locator| locator.css.as_deref())
            .any(|css| css.contains("data-figma-node-id"));
        let missing = [&check.subject, &check.container]
            .into_iter()
            .flatten()
            .filter_map(|locator| locator.css.as_deref())
            .filter(|css| css.contains("data-figma-node-id"))
            .any(|css| {
                css.split('"')
                    .enumerate()
                    .filter(|(index, _)| index % 2 == 1)
                    .all(|(_, id)| {
                        !anchors.contains(id)
                            && !id
                                .split_once(':')
                                .is_some_and(|(_, node)| anchors.contains(node))
                    })
            });
        let completeness_missing = check.completeness.as_ref().is_some_and(|complete| {
            complete
                .sections
                .iter()
                .chain(&complete.images)
                .any(|id| !anchors.contains(id) && !complete.node_selectors.contains_key(id))
        });
        if (anchored && missing) || completeness_missing {
            check.unavailable_reason = Some("not-run: semantic rendered targets absent; map nodes explicitly in the app's .devup/rendered-targets.json. Keep application JSX free of Figma attributes. DOM order is not a verified mapping.".into());
            check.subject = None;
            check.container = None;
        }
    }
}

fn dev_server_command(root: &Path, app: &str, base_url: &str) -> Value {
    let cwd = root.join(app);
    let package = std::fs::read(cwd.join("package.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let Some(script) = package
        .as_ref()
        .and_then(|package| package["scripts"]["dev"].as_str())
    else {
        return json!({"available": false, "cwd": cwd, "reason": "The route app has no package.json dev script; no start command was guessed."});
    };
    let manager = package
        .as_ref()
        .and_then(|package| package["packageManager"].as_str())
        .and_then(|value| value.split('@').next())
        .filter(|manager| matches!(*manager, "bun" | "npm" | "pnpm" | "yarn"))
        .unwrap_or_else(|| {
            if ["bun.lock", "bun.lockb"]
                .iter()
                .any(|name| cwd.join(name).exists() || root.join(name).exists())
            {
                "bun"
            } else if cwd.join("pnpm-lock.yaml").exists() || root.join("pnpm-lock.yaml").exists() {
                "pnpm"
            } else if cwd.join("yarn.lock").exists() || root.join("yarn.lock").exists() {
                "yarn"
            } else {
                "npm"
            }
        });
    json!({"available": true, "command": [manager, "run", "dev"], "cwd": cwd, "script": script,
        "windowsCommand": format!("cmd.exe /d /s /c \"{manager} run dev\""),
        "readyUrl": base_url, "readyTimeoutMs": 120000, "reuseExisting": true,
        "instructions": "Run command in cwd only when readyUrl is unreachable; wait for HTTP 200 before the runner. Stop only the process tree you started after the audit."})
}
