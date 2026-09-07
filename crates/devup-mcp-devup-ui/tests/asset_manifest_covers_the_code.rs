//! Every picture the generated code points at has to be one the manifest
//! lists.
//!
//! The code refers to an asset by path; the manifest is what tells a caller
//! to export it. Anything the code points at that the manifest does not list
//! can never be delivered, and the screen renders with a hole where the
//! picture belongs — which is exactly what happened to five photographs on
//! the about page, painted as backgrounds on layout boxes the asset walk
//! stepped straight past.
//!
//! The captures are not committed, so this checks whatever is present and
//! says so rather than pretending to have checked.

use std::{collections::BTreeSet, fs, path::PathBuf};

use devup_mcp_devup_ui::codegen::{
    CodegenOptions, asset_path, generate_component, image_fill_path,
};
use devup_mcp_figma::{Snapshot, UpstreamResult, discover_asset_manifest};

fn captures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/local-screens")
}

/// Every `/icons/x.svg` and `/images/x.png` the code draws from.
///
/// A name with a space in it is written quoted, and only then, so a quoted
/// path runs to its closing quote — `'/images/Frame 269.png'` keeps its
/// space — while a bare one ends at the first quote, bracket, comma or space
/// that follows it.
fn pointed_at(code: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for prefix in ["/icons/", "/images/"] {
        let mut rest = code;
        while let Some(at) = rest.find(prefix) {
            let after = at + prefix.len();
            let quote = rest[..at]
                .chars()
                .next_back()
                .filter(|mark| *mark == '\'' || *mark == '"');
            let end = match quote {
                // A quoted name keeps its spaces, and never holds a bracket:
                // where one turns up first the quote belonged to something
                // around the `url(...)`, not to the name.
                Some(mark) => rest[after..]
                    .find([mark, ')'])
                    .map_or(rest.len(), |offset| after + offset),
                None => rest[after..]
                    .find(['"', '\'', ')', ',', ' ', '\n'])
                    .map_or(rest.len(), |offset| after + offset),
            };
            found.insert(format!("{prefix}{}", &rest[after..end]));
            rest = &rest[end..];
        }
    }
    found
}

/// Where the manifest says each asset's bytes belong.
fn listed(snapshot: &Snapshot, per_node: bool) -> BTreeSet<String> {
    discover_asset_manifest(snapshot)
        .assets
        .iter()
        .filter_map(|asset| {
            match asset
                .field
                .strip_prefix("fills/")
                .and_then(|index| index.parse::<usize>().ok())
            {
                Some(fill_index) => image_fill_path(snapshot, &asset.node_id, fill_index, per_node),
                None => asset_path(snapshot, &asset.node_id, per_node),
            }
        })
        .collect()
}

fn snapshot_of(path: &std::path::Path) -> Option<(Snapshot, CodegenOptions)> {
    let raw = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let snapshot: Snapshot = serde_json::from_value(value.get("snapshot")?.clone()).ok()?;
    let resource = |name: &str| {
        value
            .get("payload")
            .and_then(|payload| payload.get(name))
            .cloned()
            .map(|raw| UpstreamResult { raw })
    };
    let options = CodegenOptions {
        inline_instances: true,
        ..CodegenOptions::default()
    }
    .with_resource_results(resource("variables").as_ref(), resource("styles").as_ref());
    Some((snapshot, options))
}

#[test]
fn every_asset_the_code_points_at_is_one_the_manifest_lists() {
    let Ok(entries) = fs::read_dir(captures()) else {
        eprintln!("no captures; skipping");
        return;
    };
    let mut checked = 0;
    let mut roots = 0;
    let mut failures = BTreeSet::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Some((snapshot, options)) = snapshot_of(&path) else {
            continue;
        };
        let capture = path.file_stem().unwrap_or_default().to_string_lossy();
        checked += 1;
        // The naming is an option, and the invariant holds either way: the
        // manifest and the code have to agree on the name whichever one is in
        // force.
        for per_node in [false, true] {
            let known = listed(&snapshot, per_node);
            for root in &snapshot.roots {
                let options = CodegenOptions {
                    asset_names_per_node: per_node,
                    ..options.clone()
                };
                let Ok(output) = generate_component(&snapshot, root, &options) else {
                    continue;
                };
                roots += 1;
                for wanted in pointed_at(&output.tsx) {
                    if !known.contains(&wanted) {
                        failures.insert(format!("{capture}: {wanted}"));
                    }
                }
            }
        }
    }
    // What the manifest does not list today, per capture. Every one of these
    // is a picture the code draws and nothing can deliver:
    //
    //   * a raster painted from a `PATTERN` fill, which the code sends to the
    //     icon folder and the asset walk does not visit at all;
    //   * a vector shape — ellipse, star, polygon, plain vector — drawn from
    //     a file the walk never reaches, because an ancestor answered for the
    //     subtree while the code named the child.
    //
    // Recorded rather than hidden: a capture that grows a new one fails here,
    // and a capture that loses one says so, which is the cue to lower its
    // figure. Fixing these is what removes them.
    let known: &[(&str, usize)] = &[
        ("testcase-Circle", 1),
        ("testcase-DecorativeText", 1),
        ("testcase-FlexWithMaxW", 1),
        ("testcase-MixBlendMode", 1),
        ("testcase-MixBlendModeFill", 2),
        ("testcase-ObjectFit", 2),
        ("testcase-Shapes", 3),
        ("testcase-Svg", 1),
        ("testcase-SvgDetail", 4),
    ];
    let mut counted: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for failure in &failures {
        let capture = failure.split(':').next().unwrap_or_default();
        *counted.entry(capture).or_default() += 1;
    }
    let allowed = |capture: &str| {
        known
            .iter()
            .find(|(name, _)| *name == capture)
            .map_or(0, |(_, count)| *count)
    };
    let mut worse = Vec::new();
    for (capture, count) in &counted {
        if *count > allowed(capture) {
            worse.push(format!(
                "{capture}: {count} unlisted, was {}",
                allowed(capture)
            ));
        }
    }
    for (capture, count) in known {
        let now = counted.get(capture).copied().unwrap_or(0);
        if now < *count {
            eprintln!(
                "{capture} now lists {} more of its pictures; lower its figure",
                count - now
            );
        }
    }
    assert!(
        worse.is_empty(),
        "a picture the code draws is no longer listed:\n  {}\n\nall unlisted today:\n  {}",
        worse.join("\n  "),
        failures.iter().cloned().collect::<Vec<_>>().join("\n  ")
    );
    eprintln!(
        "checked {checked} captures, {roots} generated modules, {} unlisted",
        failures.len()
    );
}
