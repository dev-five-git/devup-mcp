#![allow(dead_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use devup_mcp_devup_ui::{
    codegen::{
        CodegenOptions, generate_component, generate_component_set_target,
        generate_inlined_component_instance, generate_legacy_component, generate_node,
        render_codegen_provider, render_component_registration_snapshot, render_component_source,
        render_responsive_component_mock, render_variant_tree_merge, render_viewport_component,
    },
    theme::{ThemeScope, generate_devup_json, variable_snapshot_from_result},
};
use devup_mcp_figma::{CollectedPayload, DevupError};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixtureCase {
    pub schema_version: u32,
    pub id: String,
    pub operation: FixtureOperation,
    pub source: FixtureSource,
    pub request: FixtureRequest,
    pub payload: CollectedPayload,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureOperation {
    Tsx,
    ResponsiveTsx,
    DevupJson,
    Snapshot,
    ComponentRegistration,
    Error,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixtureSource {
    pub repository: String,
    pub commit: String,
    pub test_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixtureRequest {
    pub root_id: String,
    #[serde(default)]
    pub component_name: Option<String>,
    #[serde(default)]
    pub scope: Option<ThemeScope>,
    /// Exact non-node input used by legacy renderer/registration unit snapshots.
    /// It is kept outside `payload` so internal JavaScript test shapes are never
    /// misrepresented as fields returned by the Figma Plugin API.
    #[serde(default)]
    pub operation_input: Option<Value>,
}

#[derive(Debug)]
pub enum FixtureError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Invalid(String),
}

impl std::fmt::Display for FixtureError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "failed to read fixture: {error}"),
            Self::Json(error) => write!(formatter, "fixture JSON is invalid: {error}"),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for FixtureError {}

pub fn load_case(path: impl AsRef<Path>) -> Result<FixtureCase, FixtureError> {
    let bytes = fs::read(path).map_err(FixtureError::Io)?;
    let case: FixtureCase = serde_json::from_slice(&bytes).map_err(FixtureError::Json)?;
    validate_case(&case)?;
    Ok(case)
}

fn validate_case(case: &FixtureCase) -> Result<(), FixtureError> {
    if case.schema_version != 1 {
        return Err(FixtureError::Invalid(format!(
            "unsupported fixture schemaVersion: {}",
            case.schema_version
        )));
    }
    if case.id.trim().is_empty() || case.source.test_id.trim().is_empty() {
        return Err(FixtureError::Invalid(
            "fixture id and source.testId must not be empty.".to_owned(),
        ));
    }
    if case.source.commit.len() != 40
        || !case
            .source
            .commit
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(FixtureError::Invalid(
            "source.commit must be a 40-character git SHA.".to_owned(),
        ));
    }
    if !case
        .payload
        .snapshot
        .nodes
        .contains_key(&case.request.root_id)
    {
        return Err(FixtureError::Invalid(format!(
            "rootId '{}' is missing from the payload.",
            case.request.root_id
        )));
    }
    Ok(())
}

pub fn run_case(case: &FixtureCase) -> Result<Value, DevupError> {
    match case.operation {
        FixtureOperation::Tsx
        | FixtureOperation::ResponsiveTsx
        | FixtureOperation::Snapshot
        | FixtureOperation::ComponentRegistration => {
            let output = generate_component(
                &case.payload.snapshot,
                &case.request.root_id,
                &CodegenOptions {
                    component_name: case.request.component_name.clone(),
                    include_diagnostics: true,
                    ..CodegenOptions::default()
                }
                .with_payload_tokens(&case.payload),
            )?;
            Ok(json!({
                "tsx": output.tsx,
                "imports": output.imports,
                "usedTokens": output.used_tokens,
                "diagnostics": output.diagnostics,
            }))
        }
        FixtureOperation::DevupJson => {
            let variables = case.payload.variables.as_ref().ok_or_else(|| {
                DevupError::new(
                    devup_mcp_figma::ErrorCode::DevupThemeConflict,
                    "devup-json fixture requires a variables payload.",
                    false,
                )
            })?;
            let snapshot = variable_snapshot_from_result(variables)?;
            let output =
                generate_devup_json(&snapshot, case.request.scope.unwrap_or(ThemeScope::File))?;
            Ok(json!({
                "json": output.json,
                "counts": output.counts,
                "completeness": output.completeness,
                "diagnostics": output.diagnostics,
            }))
        }
        FixtureOperation::Error => match generate_component(
            &case.payload.snapshot,
            &case.request.root_id,
            &CodegenOptions::default(),
        ) {
            Ok(output) => Ok(json!({ "unexpectedSuccess": output.tsx })),
            Err(error) => serde_json::to_value(error).map_err(|_| {
                DevupError::new(
                    devup_mcp_figma::ErrorCode::DevupCodegenFailed,
                    "Failed to serialize the error fixture result.",
                    false,
                )
            }),
        },
    }
}

pub fn run_upstream_snapshot(case: &FixtureCase) -> Result<String, DevupError> {
    let options = CodegenOptions {
        component_name: case.request.component_name.clone(),
        include_diagnostics: true,
        ..CodegenOptions::default()
    }
    .with_payload_tokens(&case.payload);
    if let Some(input) = case.request.operation_input.as_ref() {
        if let Some(instance) = input.get("inlineComponentInstance").and_then(Value::as_str) {
            let output = generate_inlined_component_instance(
                &case.payload.snapshot,
                &case.request.root_id,
                instance,
                &options,
            )?;
            return Ok(format!("\"{}\"", output.tsx));
        }
        if input.get("treesByVariant").is_some()
            && let Some(output) = render_variant_tree_merge(input)
        {
            return Ok(output);
        }
        if input.get("mockTree").is_some()
            && let Some(output) = render_responsive_component_mock(input)
        {
            return Ok(output);
        }
        if input.get("type").and_then(Value::as_str) == Some("COMPONENT_SET")
            && let Some(output) = render_viewport_component(input)
        {
            return Ok(output);
        }
        if input.get("language").is_some()
            || input.get("type").and_then(Value::as_str) == Some("SECTION")
        {
            let pure = generate_node(&case.payload.snapshot, &case.request.root_id, &options)?;
            if let Some(output) = render_codegen_provider(input, &pure.tsx) {
                return Ok(output);
            }
        }
        if let Some(target) = input.get("targetComponent").and_then(Value::as_str)
            && matches!(case.operation, FixtureOperation::Tsx)
        {
            let output = generate_component_set_target(
                &case.payload.snapshot,
                &case.request.root_id,
                target,
                &options,
            )?;
            return Ok(format!("\"{}\"", output.tsx));
        }
        if let Some(target) = input.get("targetComponent").and_then(Value::as_str)
            && matches!(case.operation, FixtureOperation::ComponentRegistration)
        {
            return render_component_registration_snapshot(
                &case.payload.snapshot,
                &case.request.root_id,
                target,
            );
        }
        if let (Some(name), Some(code), Some(variants)) = (
            input.get("name").and_then(Value::as_str),
            input.get("code").and_then(Value::as_str),
            input.get("variants").and_then(Value::as_object),
        ) {
            let variant_order = input
                .get("variantOrder")
                .and_then(Value::as_array)
                .map(|order| order.iter().filter_map(Value::as_str).collect::<Vec<_>>());
            let variants = variant_order
                .map(|order| {
                    order
                        .into_iter()
                        .filter_map(|key| {
                            variants
                                .get(key)
                                .and_then(Value::as_str)
                                .map(|value| (key.to_owned(), value.to_owned()))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| {
                    variants
                        .iter()
                        .filter_map(|(key, value)| {
                            value
                                .as_str()
                                .map(|value| (key.to_owned(), value.to_owned()))
                        })
                        .collect::<Vec<_>>()
                });
            return Ok(format!(
                "\"{}\"",
                render_component_source(name, code, &variants)
            ));
        }
        return Ok("DEVUP_OPERATION_INPUT_NOT_IMPLEMENTED".to_owned());
    }
    if matches!(case.operation, FixtureOperation::ComponentRegistration) {
        return Ok("DEVUP_COMPONENT_REGISTRATION_NOT_IMPLEMENTED".to_owned());
    }
    let output = if matches!(case.operation, FixtureOperation::Tsx) {
        generate_legacy_component(&case.payload.snapshot, &case.request.root_id, &options)?
    } else {
        generate_node(&case.payload.snapshot, &case.request.root_id, &options)?
    };
    Ok(format!("\"{}\"", output.tsx))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixtureManifest {
    pub schema_version: u32,
    pub source: CorpusSource,
    pub baseline: CorpusBaseline,
    pub counts: CorpusCounts,
    pub source_test_files: Vec<String>,
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorpusSource {
    pub repository: String,
    pub commit: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorpusBaseline {
    pub test_files: usize,
    pub passed: usize,
    pub failed: usize,
    pub snapshots: usize,
    pub assertions: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorpusCounts {
    pub cases: usize,
    pub snapshots: usize,
    pub ledger_entries: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixtureLedger {
    pub schema_version: u32,
    pub entries: Vec<LedgerEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CoverageRegistry {
    pub schema_version: u32,
    pub evidence: Vec<CoverageEvidence>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CoverageEvidence {
    pub rust_test: String,
    pub source_path: String,
    pub function: String,
    pub coverage: EvidenceCoverage,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceCoverage {
    SnapshotParity,
    RepresentativeAssertion,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LedgerEntry {
    pub test_id: String,
    pub source_file: String,
    pub classification: LedgerClassification,
    #[serde(default)]
    pub fixture_ids: Vec<String>,
    pub rust_test: String,
    #[serde(default)]
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerClassification {
    RustSnapshot,
    RustAssertion,
    Contract,
    NotPorted,
    OutOfScopeWrite,
    UpstreamRuntimeOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusSummary {
    pub source_files: usize,
    pub ledger_entries: usize,
    pub cases: usize,
    pub snapshots: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageSummary {
    pub inventory_entries: usize,
    pub snapshot_parity_entries: usize,
    pub snapshot_cases: usize,
    pub representative_assertion_entries: usize,
    pub non_parity_entries: usize,
    pub not_ported_entries: usize,
}

pub fn validate_coverage_registry(root: &Path) -> Result<CoverageSummary, Vec<String>> {
    let ledger =
        read_json::<FixtureLedger>(&root.join("ledger.json")).map_err(|error| vec![error])?;
    let registry = read_json::<CoverageRegistry>(&root.join("coverage-registry.json"))
        .map_err(|error| vec![error])?;
    let mut violations = Vec::new();
    if registry.schema_version != 1 {
        violations.push("coverage registry schemaVersion must be 1.".to_owned());
    }

    let repository = root
        .parent()
        .and_then(Path::parent)
        .expect("fixture corpus must live below the repository root");
    let mut evidence_by_test = BTreeMap::new();
    for evidence in &registry.evidence {
        if evidence.rust_test.trim().is_empty()
            || evidence.source_path.trim().is_empty()
            || evidence.function.trim().is_empty()
            || evidence.description.trim().is_empty()
        {
            violations.push(format!(
                "coverage evidence field is empty: {}",
                evidence.rust_test
            ));
            continue;
        }
        if evidence_by_test
            .insert(evidence.rust_test.as_str(), evidence)
            .is_some()
        {
            violations.push(format!(
                "duplicate coverage evidence: {}",
                evidence.rust_test
            ));
        }
        let relative = Path::new(&evidence.source_path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            violations.push(format!(
                "coverage source path is unsafe: {}",
                evidence.source_path
            ));
            continue;
        }
        let source_path = repository.join(relative);
        match fs::read_to_string(&source_path) {
            Ok(source) => {
                let signature = format!("fn {}(", evidence.function);
                if let Some(index) = source.find(&signature) {
                    let prefix = source[..index]
                        .lines()
                        .rev()
                        .take(5)
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !prefix.contains("#[test]") && !prefix.contains("#[tokio::test]") {
                        violations.push(format!(
                            "coverage evidence is not an executable test function: {}",
                            evidence.rust_test
                        ));
                    }
                } else {
                    violations.push(format!(
                        "coverage evidence test symbol is missing: {} -> {}",
                        evidence.rust_test, evidence.source_path
                    ));
                }
            }
            Err(error) => violations.push(format!(
                "cannot read coverage source: {}: {error}",
                evidence.source_path
            )),
        }
    }

    let case_files = discover(root.join("cases"), "json", root, &mut violations);
    let snapshot_files = discover(root.join("snapshots"), "snap", root, &mut violations);
    let mut case_ids = BTreeSet::new();
    for relative in &case_files {
        let path = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        match load_case(&path) {
            Ok(case) => {
                case_ids.insert(case.id);
            }
            Err(error) => violations.push(format!("{relative}: {error}")),
        }
    }

    let mut snapshot_parity_entries = 0;
    let mut representative_assertion_entries = 0;
    let mut non_parity_entries = 0;
    let mut not_ported_entries = 0;
    let mut covered_case_ids = BTreeSet::new();
    for entry in &ledger.entries {
        match entry.classification {
            LedgerClassification::RustSnapshot => {
                snapshot_parity_entries += 1;
                covered_case_ids.extend(entry.fixture_ids.iter().cloned());
                match evidence_by_test.get(entry.rust_test.as_str()) {
                    Some(evidence) if evidence.coverage == EvidenceCoverage::SnapshotParity => {}
                    _ => violations.push(format!(
                        "rust_snapshot does not reference an executable snapshot parity test: {} -> {}",
                        entry.test_id, entry.rust_test
                    )),
                }
            }
            LedgerClassification::RustAssertion => {
                representative_assertion_entries += 1;
                match evidence_by_test.get(entry.rust_test.as_str()) {
                    Some(evidence)
                        if evidence.coverage == EvidenceCoverage::RepresentativeAssertion => {}
                    _ => violations.push(format!(
                        "rust_assertion does not reference a registered representative Rust test: {} -> {}",
                        entry.test_id, entry.rust_test
                    )),
                }
            }
            LedgerClassification::NotPorted => {
                not_ported_entries += 1;
                non_parity_entries += 1;
                if entry
                    .rationale
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    violations.push(format!("non-parity classification has no rationale: {}", entry.test_id));
                }
            }
            LedgerClassification::OutOfScopeWrite | LedgerClassification::UpstreamRuntimeOnly => {
                non_parity_entries += 1;
                if entry
                    .rationale
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty())
                {
                    violations.push(format!("non-parity classification has no rationale: {}", entry.test_id));
                }
                match evidence_by_test.get(entry.rust_test.as_str()) {
                    Some(evidence)
                        if evidence.coverage == EvidenceCoverage::RepresentativeAssertion => {}
                    _ => violations.push(format!(
                        "non-parity boundary does not reference a registered Rust contract: {} -> {}",
                        entry.test_id, entry.rust_test
                    )),
                }
            }
            LedgerClassification::Contract => violations.push(format!(
                "ambiguous contract classification must become executable evidence or explicit non-parity: {}",
                entry.test_id
            )),
        }
    }

    for case_id in case_ids.difference(&covered_case_ids) {
        violations.push(format!(
            "fixture not linked to a snapshot parity test: {case_id}"
        ));
    }
    for case_id in covered_case_ids.difference(&case_ids) {
        violations.push(format!(
            "coverage for a nonexistent snapshot fixture: {case_id}"
        ));
    }
    if case_files.len() != 268 || snapshot_files.len() != 268 {
        violations.push(format!(
            "snapshot parity corpus must have 268 JSON and 268 snapshot files: {}/{}",
            case_files.len(),
            snapshot_files.len()
        ));
    }

    if violations.is_empty() {
        Ok(CoverageSummary {
            inventory_entries: ledger.entries.len(),
            snapshot_parity_entries,
            snapshot_cases: case_ids.len(),
            representative_assertion_entries,
            non_parity_entries,
            not_ported_entries,
        })
    } else {
        Err(violations)
    }
}

pub fn validate_corpus(root: &Path) -> Result<CorpusSummary, Vec<String>> {
    let mut violations = Vec::new();
    let manifest = match read_json::<FixtureManifest>(&root.join("manifest.json")) {
        Ok(value) => value,
        Err(error) => return Err(vec![error]),
    };
    let ledger = match read_json::<FixtureLedger>(&root.join("ledger.json")) {
        Ok(value) => value,
        Err(error) => return Err(vec![error]),
    };
    if manifest.schema_version != 1 || ledger.schema_version != 1 {
        violations.push("manifest and ledger schemaVersion must be 1.".to_owned());
    }
    if manifest.source.commit.len() != 40
        || !manifest
            .source
            .commit
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        violations.push("manifest source.commit is not a 40-character git SHA.".to_owned());
    }
    if manifest.baseline.test_files != 54
        || manifest.baseline.passed != 978
        || manifest.baseline.failed != 0
        || manifest.baseline.snapshots != 268
        || manifest.baseline.assertions != 1_974
    {
        violations.push("pinned upstream baseline counts do not match.".to_owned());
    }
    if manifest.source_test_files.len() != manifest.baseline.test_files {
        violations.push("source test file count does not match the baseline.".to_owned());
    }
    duplicate_values(
        manifest.source_test_files.iter().map(String::as_str),
        "source test file",
        &mut violations,
    );

    let case_files = discover(root.join("cases"), "json", root, &mut violations);
    let snapshot_files = discover(root.join("snapshots"), "snap", root, &mut violations);
    let discovered = case_files
        .iter()
        .chain(snapshot_files.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    let declared = manifest
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();
    for path in discovered.difference(&declared) {
        violations.push(format!("orphan file missing from the manifest: {path}"));
    }
    for path in declared.difference(&discovered) {
        violations.push(format!(
            "manifest file that does not actually exist: {path}"
        ));
    }
    for file in &manifest.files {
        let path = root.join(file.path.replace('/', std::path::MAIN_SEPARATOR_STR));
        match fs::read(&path) {
            Ok(bytes) => {
                let actual = hex_sha256(&bytes);
                if actual != file.sha256 {
                    violations.push(format!("checksum mismatch: {}", file.path));
                }
            }
            Err(error) => violations.push(format!("{} read failed: {error}", file.path)),
        }
    }

    let mut case_ids = BTreeMap::<String, String>::new();
    for relative in &case_files {
        let path = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        match load_case(&path) {
            Ok(case) => {
                if let Some(first) = case_ids.insert(case.id.clone(), relative.clone()) {
                    violations.push(format!(
                        "duplicate fixture id '{}': {first}, {relative}",
                        case.id
                    ));
                }
            }
            Err(error) => violations.push(format!("{relative}: {error}")),
        }
    }

    let mut ledger_ids = BTreeSet::new();
    for entry in &ledger.entries {
        if !ledger_ids.insert(entry.test_id.as_str()) {
            violations.push(format!("duplicate ledger test id: {}", entry.test_id));
        }
        if entry.source_file.trim().is_empty() || entry.rust_test.trim().is_empty() {
            violations.push(format!("ledger path is empty: {}", entry.test_id));
        }
        for fixture_id in &entry.fixture_ids {
            if !case_ids.contains_key(fixture_id) {
                violations.push(format!(
                    "ledger references a missing fixture: {} -> {fixture_id}",
                    entry.test_id
                ));
            }
        }
        match entry.classification {
            LedgerClassification::RustSnapshot if entry.fixture_ids.is_empty() => {
                violations.push(format!(
                    "rust_snapshot ledger entry has no fixture: {}",
                    entry.test_id
                ))
            }
            LedgerClassification::NotPorted
            | LedgerClassification::OutOfScopeWrite
            | LedgerClassification::UpstreamRuntimeOnly
                if entry
                    .rationale
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty()) =>
            {
                violations.push(format!(
                    "classification has no rationale: {}",
                    entry.test_id
                ));
            }
            _ => {}
        }
    }

    if manifest.counts.cases != case_files.len()
        || manifest.counts.snapshots != snapshot_files.len()
        || manifest.counts.ledger_entries != ledger.entries.len()
    {
        violations.push("manifest counts do not match the discovered corpus.".to_owned());
    }
    if ledger.entries.len() != manifest.baseline.passed {
        violations
            .push("ledger entry count does not match the upstream passing test count.".to_owned());
    }

    if violations.is_empty() {
        Ok(CorpusSummary {
            source_files: manifest.source_test_files.len(),
            ledger_entries: ledger.entries.len(),
            cases: case_files.len(),
            snapshots: snapshot_files.len(),
        })
    } else {
        Err(violations)
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn discover(
    directory: PathBuf,
    extension: &str,
    root: &Path,
    violations: &mut Vec<String>,
) -> Vec<String> {
    let mut files = Vec::new();
    discover_inner(&directory, extension, root, &mut files, violations);
    files.sort();
    files
}

fn discover_inner(
    directory: &Path,
    extension: &str,
    root: &Path,
    files: &mut Vec<String>,
    violations: &mut Vec<String>,
) {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            violations.push(format!("{}: {error}", directory.display()));
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            discover_inner(&path, extension, root, files, violations);
        } else if path.extension().and_then(|value| value.to_str()) == Some(extension)
            && let Ok(relative) = path.strip_prefix(root)
        {
            files.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn duplicate_values<'a>(
    values: impl Iterator<Item = &'a str>,
    label: &str,
    violations: &mut Vec<String>,
) {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            violations.push(format!("duplicate {label}: {value}"));
        }
    }
}

pub fn hex_sha256(bytes: &[u8]) -> String {
    let mut canonical = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            canonical.push(b'\n');
            index += 2;
        } else {
            canonical.push(bytes[index]);
            index += 1;
        }
    }
    Sha256::digest(&canonical)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
