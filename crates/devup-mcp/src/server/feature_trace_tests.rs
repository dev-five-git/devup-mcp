use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "feature-trace-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let f = Self(root);
        f.write("package.json", "{}");
        f.write("app/users/page.tsx", "import { UserForm } from '../../components/UserForm'; export default function Page() { return <UserForm />; }");
        f.write("components/UserForm.tsx", "'use client'; export function UserForm(props: { email: string }) { api.post('createUser'); return <input name=\"email\" />; }");
        f.write("src/routes/users.rs", "schema_type!(UserInput from crate::models::users::Model, pick = [email]);\n#[vespera::route(post)]\npub async fn create() {}");
        f.write("models/users.json", r#"{"name":"users","columns":[{"name":"email","type":"String"},{"name":"secret","type":"String"}]}"#);
        let request = json!({"type":"object","properties":{"email":{"type":"string"}}});
        let response = json!({"type":"object","properties":{"id":{"type":"string"},"email":{"type":"string"}}});
        let operation = json!({"operationId":"createUser", "requestBody":{"content":{"application/json":{"schema":request}}}, "responses":{"200":{"content":{"application/json":{"schema":response}}}}});
        f.write(
            "openapi.json",
            &json!({"openapi":"3.1.0","paths":{"/users":{"post":operation}}}).to_string(),
        );
        f
    }
    fn write(&self, path: &str, text: &str) {
        let p = self.0.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    async fn trace(&self, mut input: Value) -> Value {
        input["projectRoot"] = json!(self.0.to_str().unwrap());
        run(
            serde_json::from_value(input).unwrap(),
            &ArtifactStore::default(),
        )
        .await
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn route_anchor_resolves_chain_to_selected_columns() {
    let f = Fixture::new();
    let v = f.trace(json!({"routePath":"/users"})).await;
    let hops = v["chain"].as_array().unwrap();
    for kind in [
        "screen-component",
        "component-api",
        "api-handler",
        "handler-model",
        "model-columns",
    ] {
        assert!(
            hops.iter()
                .any(|h| h["kind"] == kind && h["status"] == "RESOLVED"),
            "{kind}: {v}"
        );
    }
    let columns = hops.iter().find(|h| h["kind"] == "model-columns").unwrap();
    assert_eq!(columns["evidence"]["columns"], json!(["email"]));
}

#[tokio::test]
async fn absent_api_is_unverified_with_reason() {
    let f = Fixture::new();
    f.write("openapi.json", r#"{"paths":{}}"#);
    let v = f.trace(json!({"routePath":"/users"})).await;
    assert!(
        v["chain"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["kind"] == "component-api"
                && h["status"] == "UNVERIFIED"
                && h["reason"].as_str().is_some_and(|r| !r.is_empty())),
        "{v}"
    );
}

#[tokio::test]
async fn design_names_field_missing_from_request() {
    let v = Fixture::new().trace(json!({"figmaNodeId":"1:1","operationId":"createUser","componentTsx":"export function Design() { return <UserForm><input name=\"email\"/><input name=\"phone\"/></UserForm>; }"})).await;
    assert_eq!(
        v["designContract"][0]["request"]["designFieldsNotProvided"],
        json!(["phone"])
    );
    assert_eq!(
        v["designContract"][0]["response"]["contractFieldsUnused"],
        json!(["id"])
    );
}

#[tokio::test]
async fn generated_artifacts_are_never_edit_targets() {
    let v = Fixture::new()
        .trace(json!({"operationId":"createUser"}))
        .await;
    let ownership = v["sourceOwnership"].as_array().unwrap();
    assert!(!ownership.is_empty());
    assert!(ownership.iter().any(|o| {
        o["generated"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["path"] == "openapi.json")
    }));
    assert!(
        ownership
            .iter()
            .all(|o| o["repairDirection"].as_str().unwrap().contains("regenerat"))
    );
    assert!(
        ownership
            .iter()
            .all(|o| !o["humanAuthored"].to_string().contains("openapi.json"))
    );
}

#[tokio::test]
async fn anchorless_requirement_is_refused() {
    let v = Fixture::new()
        .trace(json!({"requirement":"Build /users with createUser"}))
        .await;
    assert_eq!(v["status"], "REFUSED");
    assert!(v["message"].as_str().unwrap().contains("routePath"));
}

#[tokio::test]
async fn reuse_is_ranked_and_explained() {
    let f = Fixture::new();
    f.write(
        "components/Other.tsx",
        "export function Other(props: { email: string }) { return <div/>; }",
    );
    let v = f.trace(json!({"figmaNodeId":"1:1","componentTsx":"export function Design() { return <UserForm><input name=\"email\"/></UserForm>; }"})).await;
    assert_eq!(
        v["reuseCandidates"][0]["component"]["path"],
        "components/UserForm.tsx"
    );
    assert_eq!(v["reuseCandidates"][0]["rank"], 1);
    assert!(
        !v["reuseCandidates"][0]["matchReasons"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn states_without_proof_are_unknown() {
    let v = Fixture::new().trace(json!({"tableName":"users"})).await;
    let states = v["requiredStates"].as_array().unwrap();
    assert_eq!(states.len(), 5);
    assert!(
        states
            .iter()
            .all(|s| s["specified"] == "unknown" && s["implemented"] == "unknown")
    );
}

#[tokio::test]
async fn named_cap_reports_omitted_candidates() {
    let f = Fixture::new();
    for n in 0..5 {
        f.write(
            &format!("components/Other{n}.tsx"),
            &format!("export function Other{n}(props: {{ email: string }}) {{ return <div/>; }}"),
        );
    }
    let v = f.trace(json!({"figmaNodeId":"1:1","componentTsx":"export function Design() { return <input name=\"email\"/>; }", "maxItems":2})).await;
    assert_eq!(v["reuseCandidates"].as_array().unwrap().len(), 2);
    assert!(
        v["truncation"]["capsHit"]
            .as_array()
            .unwrap()
            .contains(&json!("maxItems"))
    );
    assert!(
        v["truncation"]["omitted"]["reuseCandidates"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[tokio::test]
async fn literal_state_evidence_and_route_boundaries_are_present() {
    let f = Fixture::new();
    f.write(
        "app/users/loading.tsx",
        "export default function Loading() { return <p>Loading</p>; }",
    );
    let v=f.trace(json!({"routePath":"/users","figmaNodeId":"1:1","componentTsx":"export const Design=()=><section data-state=\"empty\"/>;"})).await;
    let states = v["requiredStates"].as_array().unwrap();
    assert_eq!(
        states.iter().find(|s| s["state"] == "loading").unwrap()["implemented"],
        "present"
    );
    assert_eq!(
        states.iter().find(|s| s["state"] == "empty").unwrap()["specified"],
        "present"
    );
    assert_eq!(
        states
            .iter()
            .find(|s| s["state"] == "authorization")
            .unwrap()["implemented"],
        "unknown"
    );
}

#[tokio::test]
async fn byte_cap_is_reported_and_never_exceeded() {
    let v = Fixture::new()
        .trace(json!({"tableName":"users","requirement":"x".repeat(100_000)}))
        .await;
    assert!(v.to_string().len() <= 65_536);
    assert!(
        v["truncation"]["capsHit"]
            .as_array()
            .unwrap()
            .contains(&json!("maxBytes"))
    );
}

#[tokio::test]
async fn shared_handler_file_does_not_claim_per_handler_model_binding() {
    let f = Fixture::new();
    f.write("src/routes/users.rs","schema_type!(User from crate::models::users::Model, pick = [email]);\n#[vespera::route(post)]\npub async fn create() {}\n#[vespera::route(get)]\npub async fn list() {}");
    let v = f.trace(json!({"operationId":"createUser"})).await;
    assert!(
        v["chain"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["kind"] == "handler-model"
                && h["status"] == "UNVERIFIED"
                && h["reason"].as_str().unwrap().contains("Multiple handlers"))
    );
}

#[tokio::test]
async fn missing_schema_reference_never_proves_a_contract_gap() {
    let f = Fixture::new();
    f.write("openapi.json",r##"{"paths":{"/users":{"post":{"operationId":"createUser","requestBody":{"content":{"application/json":{"schema":{"$ref":"#/components/schemas/Missing"}}}}}}}}"##);
    let v=f.trace(json!({"operationId":"createUser","figmaNodeId":"1:1","componentTsx":"export const Design=()=><input name=\"phone\"/>;"})).await;
    assert_eq!(v["designContract"][0]["request"]["status"], "UNVERIFIED");
    assert!(
        v["designContract"][0]["request"]["reasons"]
            .to_string()
            .contains("Missing")
    );
}

#[tokio::test]
async fn concrete_artifacts_carry_shared_ownership_and_authored_repair_targets() {
    let v = Fixture::new().trace(json!({"routePath":"/users"})).await;
    let artifacts = v["artifacts"].as_array().unwrap();
    let spec = artifacts
        .iter()
        .find(|a| a["path"] == "openapi.json")
        .unwrap();
    assert_eq!(spec["ownership"], "generated");
    assert_eq!(spec["editTarget"], "src/routes/users.rs");
    assert!(
        spec["sourceOwnership"]["repairDirection"]
            .as_str()
            .unwrap()
            .contains("regenerat")
    );
    assert!(artifacts.iter().all(|a| a["editTarget"] != "openapi.json"));
}

#[tokio::test]
async fn cached_artifact_reuses_codegen_and_fingerprints() {
    use crate::server::artifacts::{ArtifactRequestKey, w3_tests};
    let f = Fixture::new();
    let store = ArtifactStore::default();
    let mut payload = w3_tests::payload();
    payload.snapshot.nodes.insert("1:1".into(),serde_json::from_value(json!({"id":"1:1","type":"FRAME","fields":{"name":"Screen","width":100,"height":100,"children":[]}})).unwrap());
    let artifact = store
        .insert(
            ArtifactRequestKey::from_collection(&w3_tests::request()),
            payload,
        )
        .await
        .unwrap();
    let input =
        serde_json::from_value(json!({"projectRoot":f.0,"artifactId":artifact.artifact_id}))
            .unwrap();
    let v = run(input, &store).await.unwrap();
    assert_eq!(v["designEvidence"]["source"], "cached-artifact");
    assert!(v["designEvidence"]["designFingerprints"]["1:1"].is_string());
    assert!(v["designEvidence"]["sourceMap"]["entries"].is_array());
}

#[tokio::test]
async fn non_app_loading_file_is_not_a_route_boundary() {
    let f = Fixture::new();
    f.write(
        "loading.tsx",
        "export default function Loading(){return <p/>;}",
    );
    let v = f.trace(json!({"routePath":"/users"})).await;
    assert_eq!(
        v["requiredStates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["state"] == "loading")
            .unwrap()["implemented"],
        "unknown"
    );
}

#[tokio::test]
async fn design_comparison_keeps_explicit_operation_method() {
    let f = Fixture::new();
    f.write(
        "components/UserForm.tsx",
        "export function UserForm(){api.get('listUsers');api.post('createUser');return <p/>;}",
    );
    let mut spec: Value =
        serde_json::from_str(&std::fs::read_to_string(f.0.join("openapi.json")).unwrap()).unwrap();
    spec["paths"]["/users"]["get"] = json!({"operationId":"listUsers"});
    f.write("openapi.json", &spec.to_string());
    let v=f.trace(json!({"componentPath":"components/UserForm.tsx","apiPath":"/users","method":"post","figmaNodeId":"1:1","componentTsx":"export const Design=()=><input name=\"email\"/>;"})).await;
    assert_eq!(v["designContract"].as_array().unwrap().len(), 1);
    assert_eq!(v["designContract"][0]["operation"]["method"], "POST");
}

#[tokio::test]
async fn authored_looking_files_inside_migrations_are_never_edit_targets() {
    let f = Fixture::new();
    f.write(
        "migrations/api/models/legacy.json",
        r#"{"name":"legacy","columns":[{"name":"email","type":"String"}]}"#,
    );
    f.write("migrations/api/src/routes/legacy.rs","schema_type!(Legacy from crate::models::legacy::Model, pick = [email]);\n#[vespera::route(post)]\npub async fn create() {}\n");
    f.write(
        "migrations/api/openapi.json",
        r#"{"paths":{"/legacy":{"post":{"operationId":"createLegacy"}}}}"#,
    );
    let v = f.trace(json!({"operationId":"createLegacy"})).await;
    assert!(
        v["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["editTarget"]
                .as_str()
                .is_none_or(|p| !p.contains("migrations/"))),
        "{v}"
    );
}

#[tokio::test]
async fn database_parse_failure_is_preserved_as_unverified_evidence() {
    let f = Fixture::new();
    f.write("models/users.json", "not json");
    let v = f.trace(json!({"tableName":"users"})).await;
    assert!(
        v["inventoryEvidence"]["db"]["issues"][0]["parseError"].is_string(),
        "{v}"
    );
    assert!(
        v["chain"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["kind"] == "model-columns" && h["status"] == "UNVERIFIED")
    );
}
