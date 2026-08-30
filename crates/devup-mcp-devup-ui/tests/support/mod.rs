#![allow(dead_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, generate_component},
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
        FixtureOperation::Tsx | FixtureOperation::ResponsiveTsx | FixtureOperation::Snapshot => {
            let output = generate_component(
                &case.payload.snapshot,
                &case.request.root_id,
                &CodegenOptions {
                    component_name: case.request.component_name.clone(),
                    include_diagnostics: true,
                },
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
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
