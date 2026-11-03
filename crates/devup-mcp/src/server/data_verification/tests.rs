use super::*;
struct Fixture {
    root: std::path::PathBuf,
    policy: Option<OutputPolicy>,
    digest: String,
    receipt: Value,
    plan_path: String,
    path: String,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "devup-data-proof-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(root.join(".devup/audit")).unwrap();
        let api = json!({"paths":{"/patents":{"get":{"operationId":"listPatents","responses":{"200":{"content":{"application/json":{"schema":{"type":"object","properties":{"total":{"type":"integer"}}}}}}}}},"/scenario":{"post":{"operationId":"setScenario"}}}});
        let model = json!({"name":"patent","columns":[{"name":"id"}]});
        std::fs::write(root.join("openapi.json"), api.to_string()).unwrap();
        std::fs::write(root.join("patent.json"), model.to_string()).unwrap();
        std::fs::write(
            root.join("Screen.tsx"),
            "export function Screen(){return <Text>{data.total}</Text>}",
        )
        .unwrap();
        let policy = OutputPolicy::from_roots(vec![root.clone()]).unwrap();
        let source_digest = super::super::completion::source_digest(&root, &policy)
            .unwrap()
            .0;
        let phases=PHASES.map(|phase|json!({"phase":phase,"mutation":{"operationId":"setScenario","method":"POST","path":"/scenario","body":{"phase":phase}}}));
        let views=["landing","list","detail"].map(|surface|json!({"surface":surface,"route":format!("/{surface}"),"selector":"output","format":"{value} patents","emptyText":"No patents","errorText":"Unavailable"}));
        let binding = json!({"id":"patents.total","operationId":"listPatents","api":{"path":"/patents","pointer":"/total"},"db":{"table":"patent","field":"id","aggregate":"count","modelPath":"patent.json"},"phases":phases,"views":views});
        let plan_input:super::super::runtime_audit::PlanInput=serde_json::from_value(json!({"baseUrl":"http://127.0.0.1:3000","allowedOrigins":["http://127.0.0.1:3000"],"sourceDigest":source_digest,"screens":[{"id":"home","routePath":"/","checks":[{"kind":"data-binding","dataBinding":{"projectRoot":root,"openapiPath":"openapi.json","appBaseUrl":"http://127.0.0.1:3000","isolation":{"disposableRoot":root,"databasePath":root.join("db.sqlite"),"apiBaseUrl":"http://127.0.0.1:3000","handshakePath":"/isolation"},"bindings":[binding]}}]}]})).unwrap();
        let plan_id = super::super::runtime_audit::plan_id(&plan_input).unwrap();
        let plan = json!({"kind":"devup-runtime-audit-plan","schemaVersion":1,"planId":plan_id,"plan":plan_input});
        let plan_bytes = serde_json::to_vec(&plan).unwrap();
        let plan_path = root.join(".devup/audit/plan.json");
        std::fs::write(&plan_path, &plan_bytes).unwrap();
        let mut observations = Vec::new();
        for (index, phase) in PHASES.iter().enumerate() {
            let count = [2, 5, 0, 0][index];
            let db_path = root.join(format!(".devup/audit/{phase}.sqlite"));
            {
                let db = Connection::open(&db_path).unwrap();
                db.execute("CREATE TABLE patent(id INTEGER PRIMARY KEY)", [])
                    .unwrap();
                for id in 0..count {
                    db.execute("INSERT INTO patent(id) VALUES(?)", [id])
                        .unwrap();
                }
            }
            let observed_views=views.iter().map(|view|json!({"surface":view["surface"],"route":view["route"],"selector":"output","text":if *phase=="error"{"Unavailable".to_owned()}else if *phase=="empty"{"No patents".to_owned()}else{format!("{count} patents")}})).collect::<Vec<_>>();
            observations.push(json!({"phase":phase,"mutation":{"operationId":"setScenario","status":200,"isolationNonce":"isolated-nonce-0123456789012345"},"api":{"status":if *phase=="error"{503}else{200},"body":{"total":count},"pointer":"/total"},"db":{"adapter":"node-sqlite-readonly","query":"SELECT COUNT(\"id\") AS value FROM \"patent\"","params":[],"rows":[{"value":count}],"snapshot":{"path":format!("{phase}.sqlite"),"sha256":digest(&std::fs::read(&db_path).unwrap())}},"views":observed_views}));
        }
        let receipt = json!({"kind":"devup-data-binding-receipt","schemaVersion":1,"planId":plan_id,"planSha256":digest(&plan_bytes),"sourceDigest":source_digest,"bindings":[{"id":"patents.total","operationId":"listPatents","table":"patent","field":"id","aggregate":"count","sourceAnchor":{"openapiPath":"openapi.json","openapiSha256":digest(api.to_string().as_bytes()),"dbModelPath":"patent.json","dbSchemaSha256":digest(model.to_string().as_bytes()),"matchedPath":"/patents","matchedMethod":"get"},"phases":observations}]});
        let path = root
            .join(".devup/audit/receipt.json")
            .to_string_lossy()
            .into();
        Self {
            root,
            policy: Some(policy),
            digest: source_digest,
            receipt,
            plan_path: plan_path.to_string_lossy().into(),
            path,
        }
    }
    fn verify(&self) -> Result<Value, DevupError> {
        std::fs::write(&self.path, self.receipt.to_string()).unwrap();
        verify_data_evidence(
            self.policy.as_ref().unwrap(),
            &self.path,
            &self.digest,
            &self.plan_path,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.policy.take());
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn actual_sqlite_snapshots_are_requeried_and_api_ui_phases_recomputed() {
    let fixture = Fixture::new();
    let result = fixture.verify().unwrap();
    assert_eq!(result["verified"], true, "{result}");
    assert_eq!(result["coveredBindingIds"], json!(["patents.total"]));
    assert_eq!(result["databaseReexecuted"], true);
}
#[test]
fn api_only_or_forged_db_rows_never_claim_data_verified() {
    let mut fixture = Fixture::new();
    fixture.receipt["bindings"][0]["phases"][1]["db"]["rows"] = json!([{"value":900}]);
    let result = fixture.verify().unwrap();
    assert_eq!(result["verified"], false);
    assert_eq!(result["bindings"][0]["dbVerified"], false);
    fixture.receipt["bindings"][0]["phases"][1]["db"] = Value::Null;
    assert_eq!(fixture.verify().unwrap()["verified"], false);
}
#[test]
fn changed_sources_missing_error_and_injected_query_block_verification() {
    let mut fixture = Fixture::new();
    fixture.receipt["bindings"][0]["phases"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert_eq!(fixture.verify().unwrap()["verified"], false);
    let mut fixture = Fixture::new();
    fixture.receipt["bindings"][0]["phases"][0]["db"]["query"] = json!("DELETE FROM patent");
    assert_eq!(fixture.verify().unwrap()["verified"], false);
    std::fs::write(
        fixture.root.join("Screen.tsx"),
        "export function Screen(){return <Text>Hardcoded</Text>}",
    )
    .unwrap();
    let result = fixture.verify().unwrap();
    assert_eq!(result["bindings"][0]["sourceMatched"], false);
}
