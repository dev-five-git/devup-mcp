#![allow(dead_code)]

use std::{fs, path::Path};

use devup_mcp_devup_ui::{
    codegen::{CodegenOptions, generate_component},
    theme::{ThemeScope, generate_devup_json, variable_snapshot_from_result},
};
use devup_mcp_figma::{CollectedPayload, DevupError};
use serde::Deserialize;
use serde_json::{Value, json};

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
