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
            Self::Io(error) => write!(formatter, "fixture를 읽지 못했습니다: {error}"),
            Self::Json(error) => write!(formatter, "fixture JSON이 올바르지 않습니다: {error}"),
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
            "지원하지 않는 fixture schemaVersion입니다: {}",
            case.schema_version
        )));
    }
    if case.id.trim().is_empty() || case.source.test_id.trim().is_empty() {
        return Err(FixtureError::Invalid(
            "fixture id와 source.testId는 비어 있을 수 없습니다.".to_owned(),
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
            "source.commit은 40자리 git SHA여야 합니다.".to_owned(),
        ));
    }
    if !case
        .payload
        .snapshot
        .nodes
        .contains_key(&case.request.root_id)
    {
        return Err(FixtureError::Invalid(format!(
            "rootId '{}'가 payload에 없습니다.",
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
                    "devup-json fixture에는 variables payload가 필요합니다.",
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
                    "오류 fixture 결과를 직렬화하지 못했습니다.",
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
        violations.push("manifest와 ledger schemaVersion은 1이어야 합니다.".to_owned());
    }
    if manifest.source.commit.len() != 40
        || !manifest
            .source
            .commit
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        violations.push("manifest source.commit이 40자리 git SHA가 아닙니다.".to_owned());
    }
    if manifest.baseline.test_files != 54
        || manifest.baseline.passed != 978
        || manifest.baseline.failed != 0
        || manifest.baseline.snapshots != 268
        || manifest.baseline.assertions != 1_974
    {
        violations.push("고정 upstream baseline 수치가 일치하지 않습니다.".to_owned());
    }
    if manifest.source_test_files.len() != manifest.baseline.test_files {
        violations.push("source test file 수가 baseline과 일치하지 않습니다.".to_owned());
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
        violations.push(format!("manifest에 없는 orphan 파일: {path}"));
    }
    for path in declared.difference(&discovered) {
        violations.push(format!("실제로 존재하지 않는 manifest 파일: {path}"));
    }
    for file in &manifest.files {
        let path = root.join(file.path.replace('/', std::path::MAIN_SEPARATOR_STR));
        match fs::read(&path) {
            Ok(bytes) => {
                let actual = hex_sha256(&bytes);
                if actual != file.sha256 {
                    violations.push(format!("checksum 불일치: {}", file.path));
                }
            }
            Err(error) => violations.push(format!("{} 읽기 실패: {error}", file.path)),
        }
    }

    let mut case_ids = BTreeMap::<String, String>::new();
    for relative in &case_files {
        let path = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        match load_case(&path) {
            Ok(case) => {
                if let Some(first) = case_ids.insert(case.id.clone(), relative.clone()) {
                    violations.push(format!(
                        "중복 fixture id '{}': {first}, {relative}",
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
            violations.push(format!("중복 ledger test id: {}", entry.test_id));
        }
        if entry.source_file.trim().is_empty() || entry.rust_test.trim().is_empty() {
            violations.push(format!("ledger 경로가 비어 있습니다: {}", entry.test_id));
        }
        for fixture_id in &entry.fixture_ids {
            if !case_ids.contains_key(fixture_id) {
                violations.push(format!(
                    "ledger가 없는 fixture를 참조합니다: {} -> {fixture_id}",
                    entry.test_id
                ));
            }
        }
        match entry.classification {
            LedgerClassification::RustSnapshot if entry.fixture_ids.is_empty() => {
                violations.push(format!(
                    "rust_snapshot ledger에 fixture가 없습니다: {}",
                    entry.test_id
                ))
            }
            LedgerClassification::OutOfScopeWrite | LedgerClassification::UpstreamRuntimeOnly
                if entry
                    .rationale
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty()) =>
            {
                violations.push(format!("분류 근거가 없습니다: {}", entry.test_id));
            }
            _ => {}
        }
    }

    if manifest.counts.cases != case_files.len()
        || manifest.counts.snapshots != snapshot_files.len()
        || manifest.counts.ledger_entries != ledger.entries.len()
    {
        violations.push("manifest counts가 발견된 corpus와 일치하지 않습니다.".to_owned());
    }
    if ledger.entries.len() != manifest.baseline.passed {
        violations
            .push("ledger entry 수가 upstream passing test 수와 일치하지 않습니다.".to_owned());
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
            violations.push(format!("중복 {label}: {value}"));
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
