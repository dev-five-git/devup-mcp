use devup_mcp_figma::DevupError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct DataBindingInput {
    /// Consumer project whose complete source/assets digest is bound to the plan.
    pub project_root: String,
    /// Existing OpenAPI JSON, relative to projectRoot. No endpoints are guessed.
    pub openapi_path: String,
    /// Explicit disposable app URL on a loopback origin.
    pub app_base_url: String,
    pub isolation: IsolationInput,
    pub bindings: Vec<BindingInput>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct IsolationInput {
    /// A directory inside the OS temp directory, containing .devup-isolated.json.
    /// Marker: kind=devup-isolated-data-test, random nonce (24..128 chars),
    /// databasePath, apiOrigin and appOrigin. Symlinks cannot escape this root.
    pub disposable_root: String,
    /// Existing disposable SQLite DB. The runner opens it read-only.
    pub database_path: String,
    pub api_base_url: String,
    /// GET route returning the same marker nonce and canonical databasePath.
    pub handshake_path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct BindingInput {
    /// Exact behaviorPlan dataBindings ID (including nodeId::role when present).
    /// This is sidecar metadata and never belongs in application JSX or public paths.
    pub id: String,
    pub operation_id: String,
    pub api: ApiInput,
    pub db: DbInput,
    pub views: Vec<ViewInput>,
    /// Required in order: baseline, mutated, empty, error. Each mutation must
    /// target a documented operation of the disposable loopback API.
    pub phases: Vec<PhaseInput>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct ApiInput {
    pub path: String,
    /// JSON pointer to an existing response field.
    pub pointer: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct DbInput {
    /// Existing model JSON {name,columns:[{name,...}]}, relative to projectRoot.
    pub model_path: String,
    pub table: String,
    pub field: String,
    #[schemars(extend("enum"=["count","sum","first"]))]
    pub aggregate: String,
    /// Equality filters only. Scalar parameters are bound, never pasted into SQL.
    #[serde(default)]
    #[schemars(schema_with = "scalar_filters_schema")]
    pub r#where: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct ViewInput {
    #[schemars(extend("enum"=["landing","list","detail"]))]
    pub surface: String,
    pub route: String,
    /// Semantic CSS selector that must match exactly one visible element.
    pub selector: String,
    /// Include {value}; the value comes from the independently observed DB.
    pub format: String,
    pub empty_text: String,
    pub error_text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct PhaseInput {
    #[schemars(extend("enum"=["baseline","mutated","empty","error"]))]
    pub phase: String,
    pub mutation: MutationInput,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = serde_json::json!({"not": {}})))]
pub struct MutationInput {
    pub operation_id: String,
    #[schemars(extend("enum"=["POST","PUT","PATCH","DELETE"]))]
    pub method: String,
    pub path: String,
    #[schemars(schema_with = "json_value_schema")]
    pub body: Value,
}

pub(super) fn validate(config: &DataBindingInput) -> Result<(), DevupError> {
    let fail = |message: &str| super::invalid(message);
    let local = |value: &str| {
        url::Url::parse(value).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && matches!(
                    url.host_str(),
                    Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
                )
                && url.username().is_empty()
                && url.password().is_none()
        })
    };
    if config.project_root.is_empty()
        || config.openapi_path.is_empty()
        || config.bindings.is_empty()
        || config.bindings.len() > 20
        || !local(&config.app_base_url)
        || !local(&config.isolation.api_base_url)
    {
        return Err(fail(
            "Data proof needs explicit project/schema anchors, loopback app/API origins and 1..20 bindings.",
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    for binding in &config.bindings {
        if binding.id.is_empty()
            || binding.id.len() > 300
            || binding.id.chars().any(char::is_control)
            || !ids.insert(&binding.id)
        {
            return Err(fail(
                "Data binding IDs must uniquely match the export behavior IDs.",
            ));
        }
        if !matches!(binding.db.aggregate.as_str(), "count" | "sum" | "first")
            || binding.db.r#where.len() > 20
            || binding.db.r#where.values().any(|value| {
                !matches!(
                    value,
                    Value::Null | Value::String(_) | Value::Bool(_) | Value::Number(_)
                )
            })
        {
            return Err(fail(
                "Only typed count/sum/first aggregates and bounded scalar equality filters are supported.",
            ));
        }
        super::query(&serde_json::to_value(&binding.db).map_err(|_| fail("Invalid DB binding."))?)?;
        if binding
            .phases
            .iter()
            .map(|phase| phase.phase.as_str())
            .collect::<Vec<_>>()
            != super::PHASES
        {
            return Err(fail(
                "Data proof requires ordered baseline/mutated/empty/error phases.",
            ));
        }
        if binding.phases.iter().any(|phase| {
            !matches!(
                phase.mutation.method.as_str(),
                "POST" | "PUT" | "PATCH" | "DELETE"
            ) || !phase.mutation.path.starts_with('/')
                || phase.mutation.path.starts_with("//")
        }) {
            return Err(fail(
                "Data mutations must use explicit isolated API paths and methods.",
            ));
        }
        if !["landing", "list", "detail"]
            .iter()
            .all(|surface| binding.views.iter().any(|view| view.surface == *surface))
            || binding.views.len() > 12
        {
            return Err(fail(
                "Each binding needs bounded landing/list/detail observations.",
            ));
        }
        if binding.views.iter().any(|view| {
            view.selector.is_empty()
                || view.selector.len() > 300
                || !view.route.starts_with('/')
                || view.route.starts_with("//")
                || !view.format.contains("{value}")
                || view.empty_text.is_empty()
                || view.error_text.is_empty()
        }) {
            return Err(fail(
                "Data views require a semantic selector, local route, {value} format and explicit empty/error copy.",
            ));
        }
    }
    Ok(())
}

fn scalar_filters_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"type":"object","additionalProperties":{"type":["string","number","boolean","null"]}})
}
fn json_value_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({})
}
