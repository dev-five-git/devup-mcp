//! Conservative three-way re-export. Only design-owned JSX attributes merge;
//! imports, handlers, API reads, text expressions and accessibility remain intact.
use super::{
    output::{OutputPolicy, OutputTransaction},
    source_identity::{self, Element},
};
use devup_mcp_figma::{DevupError, ErrorCode};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct SourceSyncInput {
    /// plan is a read-only dry run; apply requires all current file hashes.
    #[schemars(extend("enum"=["plan","apply"]))]
    pub action: String,
    pub project_root: String,
    /// Existing .devup/exports record containing the original generatedSource.
    pub baseline_record: String,
    pub incoming_tsx: String,
    /// Explicit app source files, including extracted components, relative to root.
    pub files: Vec<String>,
    #[serde(default)]
    pub expected_hashes: BTreeMap<String, String>,
    #[serde(default)]
    pub expected_plan_hash: Option<String>,
}
fn invalid(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupInvalidInput, message.into(), false)
}
pub(super) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn path(root: &Path, relative: &str) -> Result<PathBuf, DevupError> {
    if Path::new(relative).is_absolute()
        || relative
            .split(['/', '\\'])
            .any(|part| matches!(part, ".." | ""))
    {
        return Err(invalid(
            "Source paths must be explicit relative paths inside projectRoot.",
        ));
    }
    let resolved = dunce::canonicalize(root.join(relative)).map_err(|e| invalid(e.to_string()))?;
    if !resolved.starts_with(root) {
        return Err(invalid("Source path resolves outside projectRoot."));
    }
    Ok(resolved)
}
fn read(policy: &OutputPolicy, path: &Path) -> Result<String, DevupError> {
    String::from_utf8(
        policy
            .resolve(&path.to_string_lossy())?
            .read_bounded(2 * 1024 * 1024)?,
    )
    .map_err(|e| invalid(e.to_string()))
}
fn styles(element: &Element) -> BTreeMap<String, String> {
    element
        .attributes
        .iter()
        .filter(|a| owned_style(&a.name))
        .map(|a| (a.name.clone(), a.source.clone()))
        .collect()
}
pub(super) fn owned_style(name: &str) -> bool {
    devup_mcp_devup_ui::style_props::is_known_style_prop(name)
        || name.starts_with('_')
        || name == "selectors"
}
fn clean(source: &str) -> bool {
    !source_identity::inspection_comments(source)
        && !devup_mcp_devup_ui::provenance::attributes::generated_attributes(source)
            .iter()
            .any(|a| {
                a.name.starts_with("data-figma-node-")
                    || a.name.starts_with("data-source-node")
                    || (owned_style(&a.name) && a.source.contains("var(--"))
            })
}
pub(super) fn run(input: SourceSyncInput, policy: &OutputPolicy) -> Result<Value, DevupError> {
    if !matches!(input.action.as_str(), "plan" | "apply")
        || input.files.is_empty()
        || input.files.len() > 60
        || input.incoming_tsx.len() > 2 * 1024 * 1024
    {
        return Err(invalid(
            "Expected plan/apply, 1..60 files and incomingTsx <=2 MiB.",
        ));
    }
    if !clean(&input.incoming_tsx) {
        return Err(invalid(
            "Incoming TSX must use clean application code and JSX $token syntax.",
        ));
    }
    let root = dunce::canonicalize(&input.project_root).map_err(|e| invalid(e.to_string()))?;
    let record_path = path(&root, &input.baseline_record)?;
    if !input
        .baseline_record
        .replace('\\', "/")
        .starts_with(".devup/exports/")
    {
        return Err(invalid(
            "baselineRecord must be an export sidecar under .devup/exports/.",
        ));
    }
    let record_source = read(policy, &record_path)?;
    let record: Value = serde_json::from_str(&record_source).map_err(|e| invalid(e.to_string()))?;
    if record["kind"] != "devup-export-record" {
        return Err(invalid("Expected an original devup export record."));
    }
    let baseline = record["sourceSyncState"]["designBaseline"]
        .as_str()
        .or_else(|| record["generatedSource"].as_str())
        .ok_or_else(|| {
            invalid("Export record has no generatedSource; re-export to an isolated path first.")
        })?;
    let base = source_identity::inventory(baseline).map_err(invalid)?;
    let incoming = source_identity::inventory(&input.incoming_tsx).map_err(invalid)?;
    let mut sources = BTreeMap::new();
    for file in &input.files {
        if !file.ends_with(".tsx") {
            return Err(invalid(
                "Source sync accepts explicit .tsx application files only.",
            ));
        }
        let source = read(policy, &path(&root, file)?)?;
        source_identity::inventory(&source).map_err(invalid)?;
        if sources.insert(file.clone(), source).is_some() {
            return Err(invalid("Duplicate source file."));
        }
    }
    let inventory = sources
        .iter()
        .map(|(file, source)| source_identity::inventory(source).map(|nodes| (file.clone(), nodes)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(invalid)?;
    let current_matches = source_identity::match_elements(&base, &inventory);
    let incoming_matches =
        source_identity::match_elements(&base, &[("incoming".into(), incoming.clone())]);
    let same_topology = base.len() == incoming.len()
        && base.iter().zip(&incoming).all(|(old, new)| {
            old.tag == new.tag
                && old.ancestor_tags == new.ancestor_tags
                && old.children == new.children
        });
    let mut conflicts = Vec::new();
    let mut patches = BTreeMap::<String, Vec<(usize, usize, String)>>::new();
    let mut mappings = Vec::new();
    // Topology changes cannot be moved around agent-owned logic without an
    // explicit identity. Refuse them instead of replacing the component body.
    if base.len() != incoming.len() {
        conflicts.push(json!({"kind":"topology-change","reason":"Added/removed design elements require explicit structural reconciliation; no body is overwritten."}));
    }
    for ((original, current), next) in base.iter().zip(current_matches).zip(incoming_matches) {
        let old_styles = styles(original);
        let next = match next {
            Ok(v) => v,
            Err(e) => {
                if same_topology {
                    source_identity::Match {
                        file: "incoming".into(),
                        element: incoming[original.index].clone(),
                        confidence: "structural-edit",
                        reasons: vec!["unchanged-complete-design-topology"],
                    }
                } else {
                    conflicts.push(e);
                    continue;
                }
            }
        };
        let next_styles = styles(&next.element);
        let current = match current {
            Ok(v) => v,
            Err(e) => {
                if old_styles == next_styles && original.text == next.element.text {
                    mappings.push(e);
                } else {
                    conflicts.push(e);
                }
                continue;
            }
        };
        mappings.push(json!({"baselineElement":original.index,"file":current.file,"elementIndex":current.element.index,"confidence":current.confidence,"reasons":current.reasons,"semanticEvidence":current.element.evidence(),"runtimeVerified":false}));
        let actual_styles = styles(&current.element);
        if original.index != next.element.index
            || original.children != next.element.children
            || original.ancestor_tags != next.element.ancestor_tags
        {
            conflicts.push(json!({"kind":"content-or-topology-change","baselineElement":original.index,"reason":"Static copy/child topology changed; preserve agent text/API bindings and request explicit reconciliation."}));
            continue;
        }
        if original.text != next.element.text {
            match (
                &original.literal_text,
                &current.element.literal_text,
                &next.element.literal_text,
            ) {
                (Some(_), Some((start, end, _)), Some((_, _, replacement)))
                    if current.element.text == original.text =>
                {
                    patches.entry(current.file.clone()).or_default().push((
                        *start,
                        *end,
                        replacement.clone(),
                    ));
                }
                _ => {
                    conflicts.push(json!({"kind":"text-conflict","file":current.file,"elementIndex":current.element.index,"reason":"Only unchanged static literal text may accept a design copy update; API expressions and edited copy are preserved."}));
                    continue;
                }
            }
        }
        let mut names = old_styles
            .keys()
            .chain(next_styles.keys())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        // Inserts share one byte offset and are applied right-to-left. Enqueue
        // them in reverse incoming order so overlapping shorthands retain the
        // exact design precedence in the resulting opening element.
        names.sort_by_key(|name| {
            std::cmp::Reverse(
                next.element
                    .attributes
                    .iter()
                    .position(|attribute| attribute.name.as_str() == name.as_str())
                    .unwrap_or(usize::MAX),
            )
        });
        for name in names {
            let before = old_styles.get(name);
            let desired = next_styles.get(name);
            let actual = actual_styles.get(name);
            let canonical = |value: Option<&String>| {
                value.map(|value| source_identity::canonical_attribute(value))
            };
            if canonical(before) == canonical(desired) || canonical(desired) == canonical(actual) {
                continue;
            }
            if current.element.spread || canonical(actual) != canonical(before) {
                conflicts.push(json!({"kind":"style-conflict","file":current.file,"elementIndex":current.element.index,"property":name,"baseline":before,"current":actual,"incoming":desired,"reason":"Concurrent style edit or JSX spread makes precedence ambiguous."}));
                continue;
            }
            let target = current.element.attributes.iter().find(|a| &a.name == name);
            let (start, end) = target.map(|a| (a.start, a.end)).unwrap_or_else(|| {
                let source = &sources[&current.file];
                let end = current.element.end;
                let at = if source[..end].ends_with("/>") {
                    end - 2
                } else {
                    end - 1
                };
                (at, at)
            });
            let replacement = desired.map_or_else(String::new, |value| {
                if target.is_none() {
                    format!(" {value}")
                } else {
                    value.clone()
                }
            });
            patches
                .entry(current.file.clone())
                .or_default()
                .push((start, end, replacement));
        }
    }
    let hashes = sources
        .iter()
        .map(|(file, source)| (file.clone(), digest(source.as_bytes())))
        .collect::<BTreeMap<_, _>>();
    let mut proposed = BTreeMap::new();
    let mut delivered_inventory = inventory.into_iter().collect::<BTreeMap<_, _>>();
    for (file, mut edits) in patches {
        let mut source = sources[&file].clone();
        edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
        for (start, end, text) in edits {
            source.replace_range(start..end, &text);
        }
        delivered_inventory.insert(
            file.clone(),
            source_identity::inventory(&source).map_err(invalid)?,
        );
        if !clean(&source) {
            conflicts.push(json!({"kind":"unclean-current-source","file":file}));
        }
        proposed.insert(file, source);
    }
    for mapping in &mut mappings {
        if let (Some(file), Some(index)) =
            (mapping["file"].as_str(), mapping["elementIndex"].as_u64())
            && let Some(element) = delivered_inventory
                .get(file)
                .and_then(|nodes| nodes.get(index as usize))
        {
            mapping["matchEvidenceBeforeUpdate"] = mapping["semanticEvidence"].clone();
            mapping["semanticEvidence"] = element.evidence();
            mapping["evidenceScope"] = json!(
                "proposed-source-after-safe-merge; match reasons describe the original baseline-to-source match"
            );
        }
    }
    let plan_hash=digest(serde_json::to_vec(&json!({"baseline":digest(record_source.as_bytes()),"incoming":digest(input.incoming_tsx.as_bytes()),"hashes":hashes,"proposed":proposed})).map_err(|e|invalid(e.to_string()))?.as_slice());
    let applicable = conflicts.is_empty();
    let mut applied = false;
    if input.action == "apply" && applicable {
        if hashes != input.expected_hashes
            || input.expected_plan_hash.as_deref() != Some(plan_hash.as_str())
        {
            return Err(invalid(
                "Apply requires exact expectedPlanHash and expectedHashes returned by the dry run; inputs or source changed, or hashes omitted.",
            ));
        }
        let mut tx = OutputTransaction::new();
        for (file, source) in &proposed {
            tx.stage_registry(
                file,
                policy.resolve(&root.join(file).to_string_lossy())?,
                source.as_bytes(),
                Some(hashes[file].clone()),
            )?;
        }
        let sidecar = root.join(".devup/source-mappings.json");
        let previous = std::fs::read(&sidecar).ok().map(|bytes| digest(&bytes));
        let receipt = json!({"kind":"devup-source-mappings","schemaVersion":1,"baselineRecord":input.baseline_record,"baselineHash":digest(input.incoming_tsx.as_bytes()),"planHash":plan_hash,"files":sources.keys().map(|file|(file.clone(),digest(proposed.get(file).unwrap_or(&sources[file]).as_bytes()))).collect::<BTreeMap<_,_>>(),"mappings":mappings,"nodeMappings":record["nodes"],"runtimeVerified":false});
        tx.stage_registry(
            "sourceMappings",
            policy.resolve(&sidecar.to_string_lossy())?,
            serde_json::to_vec_pretty(&receipt)
                .map_err(|e| invalid(e.to_string()))?
                .as_slice(),
            previous,
        )?;
        // A caller's incoming TSX is an edit proposal, never fresh Figma
        // acquisition evidence. Retain the original generatedSource/provenance.
        let mut next_record = record.clone();
        next_record["sourceSyncState"] = json!({"designBaseline":input.incoming_tsx,"incomingProvenance":"caller-supplied-unverified","planHash":plan_hash,"sourceMappings":".devup/source-mappings.json"});
        tx.stage_registry(
            "exportBaseline",
            policy.resolve(&record_path.to_string_lossy())?,
            serde_json::to_vec_pretty(&next_record)
                .map_err(|e| invalid(e.to_string()))?
                .as_slice(),
            Some(digest(record_source.as_bytes())),
        )?;
        tx.commit()?;
        applied = true;
    }
    Ok(
        json!({"kind":"devup-source-sync","action":input.action,"dryRun":input.action=="plan","applicable":applicable,"applied":applied,"planHash":plan_hash,"expectedHashes":hashes,"proposedSources":proposed,"conflicts":conflicts,"mappings":mappings,"incomingProvenance":"caller-supplied-unverified","preserved":["imports","API/data expressions","event handlers","ARIA/accessibility","unmodified agent styles"],"runtimeVerified":false}),
    )
}

#[cfg(test)]
#[path = "source_sync/tests.rs"]
mod tests;
