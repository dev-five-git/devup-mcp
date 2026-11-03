//! Bounded asset inventories and reviewable, optimistic URL migrations.
mod apply;
mod database;
mod inventory;
mod plan;
mod references;

use super::output::{OutputPolicy, OutputTransaction};
use devup_mcp_figma::{DevupError, ErrorCode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

const MAX_FILES: usize = 10_000;
const MAX_FILE_BYTES: usize = 32 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(rename_all = "kebab-case")]
pub(super) enum AssetsAction {
    #[default]
    Inventory,
    Plan,
    Apply,
    Rollback,
}

#[derive(Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub(super) struct AssetRename {
    /// Project-relative public directory. Optional when inventory selects exactly one.
    #[serde(default)]
    pub public_root: Option<String>,
    pub from: String,
    pub to: String,
}

#[derive(Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub(super) struct DatabaseReferences {
    /// Project-relative JSON array of {key:string,url:string}, exported from the database.
    /// Snapshot freshness and live database equality are not inferred.
    pub snapshot_path: String,
    #[serde(default = "default_schema")]
    pub schema: String,
    pub table: String,
    pub key_column: String,
    pub url_column: String,
}
fn default_schema() -> String {
    "public".into()
}

#[derive(Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub(super) struct AssetsInput {
    #[serde(default)]
    pub action: AssetsAction,
    pub project_root: String,
    /// Relative public directories; omitted discovers public/ directories in this checkout.
    #[serde(default)]
    pub public_roots: Vec<String>,
    /// Additional source/seed files or directories; omitted scans authored source/data files.
    #[serde(default)]
    pub reference_paths: Vec<String>,
    #[serde(default)]
    pub renames: Vec<AssetRename>,
    #[serde(default)]
    pub database_references: Vec<DatabaseReferences>,
    /// Apply/rollback reads the exact persisted plan created by plan with dryRun:false.
    #[serde(default)]
    pub plan_path: Option<String>,
    /// Defaults true for every action. False is required to write a plan or apply/rollback it.
    #[serde(default = "yes")]
    pub dry_run: bool,
    /// Retire old local files only in an explicitly bounded static scope with no pending DB migration.
    /// Arbitrary runtime/dynamic usage remains unproven even when the static scan is clean.
    #[serde(default)]
    pub retire_sources: bool,
    /// Bounded response pages; the persisted plan contains the complete inventory.
    #[serde(default = "default_response_limit")]
    pub response_limit: usize,
    #[serde(default)]
    pub response_offset: usize,
}
fn default_response_limit() -> usize {
    100
}
fn yes() -> bool {
    true
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicAsset {
    public_root: String,
    url: String,
    path: String,
    sha256: String,
    bytes: usize,
    semantic_name: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Reference {
    file: String,
    url: String,
    start: usize,
    end: usize,
    syntax: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DynamicReference {
    file: String,
    start: usize,
    reason: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Inventory {
    public_roots: Vec<String>,
    assets: Vec<PublicAsset>,
    references: Vec<Reference>,
    dynamic_references: Vec<DynamicReference>,
    file_hashes: BTreeMap<String, String>,
    database_snapshots: Vec<Value>,
    diagnostics: Vec<Value>,
    complete: bool,
}
struct Workspace<'a> {
    root: PathBuf,
    policy: &'a OutputPolicy,
}
impl<'a> Workspace<'a> {
    fn new(project: &str, policy: &'a OutputPolicy) -> Result<Self, DevupError> {
        let root = policy
            .resolve(
                &Path::new(project)
                    .join(".devup/asset-probe")
                    .to_string_lossy(),
            )?
            .display_path()
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| invalid("Invalid projectRoot"))?
            .to_path_buf();
        if !root.is_dir() {
            return Err(invalid("projectRoot must exist"));
        }
        Ok(Self { root, policy })
    }
    fn target(&self, relative: &str) -> Result<super::output::OutputTarget, DevupError> {
        let path = Path::new(relative);
        if path.is_absolute()
            || path
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(invalid(
                "Asset operation paths must stay relative to projectRoot",
            ));
        }
        self.policy.resolve(&self.root.join(path).to_string_lossy())
    }
    fn read(&self, relative: &str) -> Result<Vec<u8>, DevupError> {
        self.target(relative)?.read_bounded(MAX_FILE_BYTES)
    }
    fn relative(&self, path: &Path) -> Result<String, DevupError> {
        Ok(path
            .strip_prefix(&self.root)
            .map_err(|_| invalid("Asset inventory escaped projectRoot"))?
            .components()
            .map(|p| p.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"))
    }
}

fn invalid(message: impl Into<String>) -> DevupError {
    DevupError::new(ErrorCode::DevupInvalidInput, message, false)
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn url_path(url: &str) -> Result<String, DevupError> {
    if !url.starts_with('/') || url.starts_with("//") || url.contains(['?', '#', '\\']) {
        return Err(invalid(
            "Asset URLs must be root-relative paths without query or fragment",
        ));
    }
    let decoded = super::mask_guard::percent_decode(url);
    if decoded
        .split('/')
        .skip(1)
        .any(|p| p.is_empty() || p == "." || p == ".." || p.contains(['\\', '\0', '\r', '\n']))
    {
        return Err(invalid("Unsafe asset URL"));
    }
    Ok(decoded.trim_start_matches('/').into())
}
fn semantic(url: &str) -> bool {
    super::projection::semantic_asset_name(url)
}

fn page<T: Serialize>(items: &[T], input: &AssetsInput) -> Value {
    json!(
        items
            .iter()
            .skip(input.response_offset)
            .take(input.response_limit)
            .collect::<Vec<_>>()
    )
}
fn bounded_page(
    input: &AssetsInput,
    build: impl Fn(&AssetsInput) -> Value,
) -> Result<Value, DevupError> {
    let mut selected = input.clone();
    loop {
        let value = build(&selected);
        if serde_json::to_vec(&value).unwrap().len() <= 200_000 {
            return Ok(value);
        }
        if selected.response_limit <= 1 {
            return Err(invalid(
                "One asset response row exceeds the delivery bound; narrow scope or use shorter labels",
            ));
        }
        selected.response_limit = selected.response_limit.div_ceil(2);
    }
}
fn inventory_reply(inventory: &Inventory, input: &AssetsInput) -> Value {
    let has_more = [
        inventory.assets.len(),
        inventory.references.len(),
        inventory.dynamic_references.len(),
        inventory.diagnostics.len(),
    ]
    .into_iter()
    .any(|n| n > input.response_offset.saturating_add(input.response_limit));
    json!({"publicRoots":inventory.public_roots,"assets":page(&inventory.assets,input),"references":page(&inventory.references,input),"dynamicReferences":page(&inventory.dynamic_references,input),"diagnostics":page(&inventory.diagnostics,input),"complete":inventory.complete,"sourceFileCount":inventory.file_hashes.len(),"databaseSnapshots":inventory.database_snapshots.iter().map(|snapshot|json!({"snapshotPath":snapshot["snapshotPath"],"sha256":snapshot["sha256"],"adapter":snapshot["adapter"],"table":snapshot["table"],"rowCount":snapshot["rowCount"],"liveDatabaseVerified":false})).collect::<Vec<_>>(),"totals":{"assets":inventory.assets.len(),"references":inventory.references.len(),"dynamicReferences":inventory.dynamic_references.len(),"diagnostics":inventory.diagnostics.len()},"paging":{"offset":input.response_offset,"limit":input.response_limit,"hasMore":has_more}})
}

pub(super) fn run(input: AssetsInput, policy: &OutputPolicy) -> Result<Value, DevupError> {
    if !(1..=200).contains(&input.response_limit)
        || input.response_offset > MAX_FILES
        || input.public_roots.len() > 64
        || input.reference_paths.len() > 256
        || input.database_references.len() > 64
    {
        return Err(invalid("Asset tool scope/page bounds exceeded"));
    }
    let workspace = Workspace::new(&input.project_root, policy)?;
    match input.action {
        AssetsAction::Inventory => {
            let inventory = inventory::scan(&workspace, &input)?;
            let duplicate_groups = inventory::duplicates(&inventory);
            bounded_page(
                &input,
                |input| json!({"action":"inventory","inventory":inventory_reply(&inventory,input),"duplicateGroups":page(&duplicate_groups,input),"duplicateGroupCount":duplicate_groups.len(),"dynamicUsageProven":false,"liveDatabaseVerified":false,"bounds":{"maxFiles":MAX_FILES,"maxFileBytes":MAX_FILE_BYTES,"maxTotalBytes":MAX_TOTAL_BYTES},"nextPage":{"tool":"devup_assets","arguments":{"action":"inventory","projectRoot":input.project_root,"publicRoots":input.public_roots,"referencePaths":input.reference_paths,"databaseReferences":input.database_references,"responseOffset":input.response_offset.saturating_add(input.response_limit),"responseLimit":input.response_limit}}}),
            )
        }
        AssetsAction::Plan => plan::run(&workspace, input),
        AssetsAction::Apply => apply::run(&workspace, &input, false),
        AssetsAction::Rollback => apply::run(&workspace, &input, true),
    }
}

#[cfg(test)]
mod tests;
