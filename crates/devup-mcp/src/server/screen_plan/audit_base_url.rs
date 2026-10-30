/// The server auditPlan targets: the call's baseUrl, else the one an earlier
/// auditPlan recorded, so a later call limited to some `urls` does not
/// silently fall back to localhost:3000 and split the plans across servers.
fn audit_base_url(input: &ScreenPlanInput, plan: &Value) -> String {
    input
        .base_url
        .clone()
        .or_else(|| plan["audit"]["baseUrl"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "http://localhost:3000".to_owned())
}

fn base_url_mismatch(entries: &[Value]) -> Option<Value> {
    let mut origins = BTreeMap::<String, BTreeSet<String>>::new();
    for entry in entries {
        if let Some(base) = entry["baseUrl"].as_str() {
            origins
                .entry(text(&entry["app"]).to_owned())
                .or_default()
                .insert(base.to_owned());
        }
    }
    let mismatched = origins
        .iter()
        .filter(|(_, bases)| bases.len() > 1)
        .map(|(app, bases)| {
            let urls = entries
                .iter()
                .filter(|entry| text(&entry["app"]) == app.as_str())
                .map(|entry| json!({"url": entry["url"], "baseUrl": entry["baseUrl"]}))
                .collect::<Vec<_>>();
            json!({"app": app, "baseUrls": bases, "urls": urls})
        })
        .collect::<Vec<_>>();
    (!mismatched.is_empty()).then(|| json!({
        "apps": mismatched,
        "fix": "The audit plans of one app point at different servers. Run auditPlan again for every URL with one baseUrl (or baseUrls per app) so each report measures the same build.",
    }))
}
