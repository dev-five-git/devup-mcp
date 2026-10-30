use super::*;

fn apps(root: &Path) -> Vec<String> {
    let mut found = vec![];
    if root.join("src/app").is_dir() { found.push(String::new()); }
    if let Ok(entries) = std::fs::read_dir(root.join("apps")) {
        for entry in entries.filter_map(Result::ok) {
            if entry.path().join("src/app").is_dir() { found.push(format!("apps/{}", entry.file_name().to_string_lossy())); }
        }
    }
    found.sort();
    found
}

fn app_for(route: &Value, apps: &[String]) -> Option<String> {
    for key in ["file", "app", "appRoot", "projectRoot"] {
        if let Some(value) = route[key].as_str() {
            let value = value.replace('\\', "/");
            if let Some(app) = apps.iter().filter(|app| !app.is_empty()).find(|app| value == **app || value.starts_with(&format!("{app}/")) || value.ends_with(&format!("/{app}")) || value.contains(&format!("/{app}/"))) { return Some(app.clone()); }
        }
    }
    if apps.len() == 1 { apps.first().cloned() } else { None }
}

fn annotated(path: String, evidence: &str) -> Value {
    json!({"path":path,"inferred":true,"evidence":evidence})
}

fn owned(route: &Value, screens: &[Value]) -> bool {
    screens.contains(&route["url"]) || array(&route["nodeIds"]).iter().any(|node| screens.contains(node))
}

fn assets(source: &Value, output: &mut Vec<Value>) {
    for key in ["assets", "assetManifest"] {
        let value = &source[key];
        let rows = if value.is_array() {array(value)} else {array(&value["assets"])};
        for asset in rows {
            let mut asset = if asset.is_object() {asset.clone()} else {json!({"assetId":asset})};
            asset["inferred"] = json!(true);
            asset["evidence"] = json!("owned screen/export asset facts");
            if !output.contains(&asset) { output.push(asset); }
        }
    }
}

pub(super) fn drafts(plan: &Value, unit: &Value, root: &str) -> Value {
    let apps = apps(Path::new(root));
    let mut result = json!({"exports":[],"assets":[],"imports":[]});
    let mut features = array(&unit["features"]).to_vec();
    if features.is_empty() { features.push(json!(unit_owner(unit))); }
    for owner in features.iter().filter_map(Value::as_str) {
        let feature = array(&plan["features"]).iter().find(|feature| feature["id"] == owner);
        let screens = feature.map(|feature| array(&feature["screens"])).unwrap_or_else(|| array(&unit["screens"]));
        let routes = array(&plan["screenPlan"]["routes"]).iter().filter(|route| owned(route, screens)).collect::<Vec<_>>();
        let mut inferred = json!({"exports":[],"assets":[],"imports":[]});
        let mut imports = vec![];
        for route in &routes {
            let Some(app) = app_for(route, &apps) else {continue};
            let prefix = if app.is_empty() {String::new()} else {format!("{app}/")};
            let segment = text(&route["url"]).trim_matches('/');
            let suffix = if segment.is_empty() {String::new()} else {format!("{segment}/")};
            if let Some(exports) = inferred["exports"].as_array_mut() {
                exports.push(annotated(format!("{prefix}src/app/{suffix}page.tsx"), "owned route and detected App Router app"));
                exports.push(annotated(format!("{prefix}src/components/pages/{suffix}"), "owned route; project page-component convention"));
            }
            for part in array(&unit["contract"]["sharedParts"]).iter().filter_map(Value::as_str) {
                let directory = if ["header", "footer", "nav"].iter().any(|name| part.to_lowercase().contains(name)) {"layout"} else {"common"};
                let item = annotated(format!("{prefix}src/components/{directory}/{part}.tsx"), "sharedParts; project shared-component convention");
                if !imports.contains(&item) { imports.push(item); }
            }
        }
        inferred["imports"] = json!(imports);
        let mut asset_rows = vec![];
        if let Some(feature) = feature {
            for screen in feature_screens(plan, feature) { assets(&screen, &mut asset_rows); }
        }
        for route in &routes { assets(route, &mut asset_rows); }
        for export in array(&plan["facts"]["exports"]).iter().chain(array(&plan["screenPlan"]["exports"])) {
            if screens.contains(&export["nodeId"]) || screens.contains(&export["screenId"]) || routes.iter().any(|route| array(&route["nodeIds"]).contains(&export["nodeId"])) {
                assets(export, &mut asset_rows);
            }
        }
        inferred["assets"] = json!(asset_rows);
        for field in ["exports", "assets", "imports"] {
            let explicit = plan["facts"].get(format!("{owner}.{field}"));
            let values = explicit.unwrap_or(&inferred[field]);
            if let Some(target) = result[field].as_array_mut() {
                for value in array(values) { if !target.contains(value) { target.push(value.clone()); } }
            }
        }
    }
    result
}
