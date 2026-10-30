fn gate_source_anchors(built: &mut UrlPlan, app: &Path) {
    fn scan(path: &Path, text: &mut String) {
        let Ok(entries) = std::fs::read_dir(path) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_symlink()) { continue; }
            if path.is_dir() { scan(&path, text); }
            else if matches!(path.extension().and_then(|ext| ext.to_str()), Some("tsx" | "jsx" | "ts" | "js"))
                && let Ok(source) = std::fs::read_to_string(path) {
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
        let (plural, tail) = tail.strip_prefix('s').map_or((false, tail), |tail| (true, tail));
        let Some(tail) = tail.trim_start().strip_prefix('=') else { continue };
        let tail = tail.trim_start();
        let Some(quote @ ('\'' | '"')) = tail.chars().next() else { continue };
        if let Some(value) = tail[1..].split(quote).next() {
            for value in value.split_whitespace() {
                let id = if plural { value.split_once(':').map_or(value, |(_, id)| id) } else { value };
                anchors.insert(id.to_owned());
            }
        }
    }
    for check in &mut built.plan.screens[0].checks {
        let anchored = [&check.subject, &check.container].into_iter().flatten()
            .filter_map(|locator| locator.css.as_deref()).any(|css| css.contains("data-figma-node-id"));
        let missing = [&check.subject, &check.container].into_iter().flatten().filter_map(|locator| locator.css.as_deref()).any(|css| {
            css.split('"').enumerate().filter(|(index, _)| index % 2 == 1)
                .all(|(_, id)| !anchors.contains(id) && !id.split_once(':').is_some_and(|(_, node)| anchors.contains(node)))
        });
        let completeness_missing = check.completeness.as_ref().is_some_and(|complete| complete.sections.iter().chain(&complete.images).any(|id| !anchors.contains(id)));
        if (anchored && missing) || completeness_missing {
            check.unavailable_reason = Some("not-run: source anchors absent; data-figma-node-id is off by default. Export with figmaNodeIds:true and preserve anchors, or use explicit role/testId locators.".into());
            check.subject = None;
            check.container = None;
        }
    }
}

fn dev_server_command(root: &Path, app: &str, base_url: &str) -> Value {
    let cwd = root.join(app);
    let package = std::fs::read(cwd.join("package.json")).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let Some(script) = package.as_ref().and_then(|package| package["scripts"]["dev"].as_str()) else {
        return json!({"available": false, "cwd": cwd, "reason": "The route app has no package.json dev script; no start command was guessed."});
    };
    let manager = package.as_ref().and_then(|package| package["packageManager"].as_str())
        .and_then(|value| value.split('@').next()).filter(|manager| matches!(*manager, "bun" | "npm" | "pnpm" | "yarn"))
        .unwrap_or_else(|| {
            if ["bun.lock", "bun.lockb"].iter().any(|name| cwd.join(name).exists() || root.join(name).exists()) { "bun" }
            else if cwd.join("pnpm-lock.yaml").exists() || root.join("pnpm-lock.yaml").exists() { "pnpm" }
            else if cwd.join("yarn.lock").exists() || root.join("yarn.lock").exists() { "yarn" }
            else { "npm" }
        });
    json!({"available": true, "command": [manager, "run", "dev"], "cwd": cwd, "script": script,
        "windowsCommand": format!("cmd.exe /d /s /c \"{manager} run dev\""),
        "readyUrl": base_url, "readyTimeoutMs": 120000, "reuseExisting": true,
        "instructions": "Run command in cwd only when readyUrl is unreachable; wait for HTTP 200 before the runner. Stop only the process tree you started after the audit."})
}
