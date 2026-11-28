use super::*;
use crate::server::artifacts::w3_tests;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::Digest as _;

const WHITE: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4////fwAJ+wP9KobjigAAAABJRU5ErkJggg==";
const BLACK: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYGD4DwABBAEAX+XDSwAAAABJRU5ErkJggg==";

struct Dir(std::path::PathBuf);
impl Dir {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "verification-matrix-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn node(value: Value) -> devup_mcp_figma::RawNode {
    serde_json::from_value(value).unwrap()
}

/// A screen frame holding a Button instance whose set declares
/// Default/Hover/Disabled variants, with a red 120x40 radius-8 fill.
fn payload() -> CollectedPayload {
    let mut payload = w3_tests::payload();
    let nodes = &mut payload.snapshot.nodes;
    nodes.insert("1:1".into(), node(json!({"id":"1:1","type":"FRAME","fields":{"name":"Screen","width":400,"height":300,
        "absoluteBoundingBox":{"x":100,"y":100,"width":400,"height":300},"childrenIds":["2:1","2:9"]}})));
    nodes.insert("2:1".into(), node(json!({"id":"2:1","type":"INSTANCE","fields":{"name":"Button","parentId":"1:1",
        "mainComponentId":"3:1","width":120,"height":40,"cornerRadius":8,
        "absoluteBoundingBox":{"x":110,"y":120,"width":120,"height":40},
        "fills":[{"type":"SOLID","color":{"r":1,"g":0,"b":0}}],"childrenIds":[]}})));
    nodes.insert("3:0".into(), node(json!({"id":"3:0","type":"COMPONENT_SET","fields":{"name":"Button","childrenIds":["3:1","3:2","3:3"]}})));
    for (id, state) in [("3:1", "Default"), ("3:2", "Hover"), ("3:3", "Disabled")] {
        nodes.insert(id.into(), node(json!({"id":id,"type":"COMPONENT","fields":{"parentId":"3:0","variantProperties":{"State":state}}})));
    }
    nodes.insert("2:9".into(), node(json!({"id":"2:9","type":"TEXT","fields":{"name":"Title","parentId":"1:1","fontSize":20,
        "fontName":{"family":"Inter","style":"Semi Bold"},"lineHeight":{"unit":"PIXELS","value":28},
        "fills":[{"type":"SOLID","color":{"r":0,"g":0,"b":0}}]}})));
    payload
}

fn ui() -> Value {
    json!({"routes":[{"route":"/a","page":"app/a/page.tsx","appliesTo":"."},{"route":"/b","page":"app/b/page.tsx","appliesTo":"."}],
        "components":[
            {"path":"components/Button.tsx","importedBy":[{"path":"app/a/page.tsx","routes":["/a"]},{"path":"app/b/page.tsx","routes":["/b"]}]},
            {"path":"components/Hero.tsx","importedBy":[{"path":"app/a/page.tsx","routes":["/a"]}]}]})
}

struct Run {
    dir: Dir,
    ui: Value,
    payload: Option<CollectedPayload>,
    unavailable: Option<String>,
    policy: Option<OutputPolicy>,
    store: ArtifactStore,
}
impl Run {
    fn new() -> Self {
        Self { dir: Dir::new(), ui: ui(), payload: Some(payload()), unavailable: None, policy: None, store: ArtifactStore::default() }
    }
    async fn matrix(&self, input: Value) -> Value {
        let input: super::super::tools::FeatureTraceInput = serde_json::from_value(input).unwrap();
        let context = MatrixContext {
            ui: &self.ui,
            root: &self.dir.0,
            payload: self.payload.as_ref(),
            artifact_unavailable: self.unavailable.clone(),
            policy: self.policy.as_ref(),
            store: &self.store,
        };
        let matrix = MatrixInput {
            state_checks: &input.state_checks,
            requirements: &input.requirements,
            design_gaps: &input.design_gaps,
            changes: &input.changes,
            component_path: input.component_path.as_deref(),
        };
        build(&context, &matrix).await.unwrap()
    }
}

fn button_values() -> Value {
    json!({"x":10,"y":20,"width":120,"height":40,"backgroundColor":"rgb(255, 0, 0)","borderRadius":8})
}
fn state<'a>(matrix: &'a Value, target: &str, state: &str) -> &'a Value {
    array(&matrix["stateGroups"])
        .iter()
        .flat_map(|g| array(&g["states"]))
        .find(|s| s["target"] == target && s["state"] == state)
        .unwrap_or_else(|| panic!("{target} {state}: {matrix}"))
}

#[tokio::test]
async fn states_are_grouped_and_missing_or_static_states_are_never_a_pass() {
    let run = Run::new();
    let m = run.matrix(json!({"stateChecks":[
        {"target":"components/Button.tsx","routePath":"/a","state":"Default","figmaNodeId":"2:1","implementation":button_values(),"implementationSource":"browser"},
        {"target":"components/Button.tsx","routePath":"/a","state":"hovered","figmaNodeId":"2:1","implementation":button_values()}
    ]})).await;
    let groups = array(&m["stateGroups"]);
    assert_eq!(groups.len(), 1, "{m}");
    assert_eq!(groups[0]["expectedStates"], json!(["default", "disabled", "hover"]));
    let default = state(&m, "components/Button.tsx", "default");
    assert_eq!(default["verdict"], "verified", "{default}");
    let categories = array(&default["comparisons"]).iter().map(|c| text(&c["category"])).collect::<BTreeSet<_>>();
    assert_eq!(categories, BTreeSet::from(["border", "color", "position", "size"]));
    let hover = state(&m, "components/Button.tsx", "hover");
    assert_eq!(hover["verdict"], "unverified");
    assert_eq!(hover["browserVerificationRequired"], true);
    let disabled = state(&m, "components/Button.tsx", "disabled");
    assert_eq!(disabled["verdict"], "unverified");
    assert_eq!(disabled["missing"], true);
    assert_eq!(groups[0]["counts"], json!({"verified":1,"failed":0,"unverified":2}));
    assert_eq!(m["claims"]["designMatch"], "blocked");
    assert_eq!(m["claims"]["verificationComplete"], "blocked");
}

#[tokio::test]
async fn mismatches_fail_with_property_deltas_and_ask_for_a_capture() {
    let run = Run::new();
    let mut values = button_values();
    values["width"] = json!(121);
    values["backgroundColor"] = json!("#00ff00");
    let m = run.matrix(json!({"stateChecks":[
        {"target":"Button","state":"default","figmaNodeId":"2:1","implementation":values,"implementationSource":"browser"},
        {"target":"Title","state":"default","figmaNodeId":"2:9","implementationSource":"browser",
            "implementation":{"fontFamily":"\"Inter\", sans-serif","fontSize":20,"fontWeight":600,"lineHeight":28.4,"color":"#000"}}
    ]})).await;
    let button = state(&m, "Button", "default");
    assert_eq!(button["verdict"], "failed");
    assert_eq!(button["mismatchCount"], 2);
    let width = array(&button["comparisons"]).iter().find(|c| c["property"] == "width").unwrap();
    assert_eq!(width["delta"], 1.0);
    assert_eq!(button["capture"]["needed"], true);
    let title = state(&m, "Title", "default");
    assert_eq!(title["verdict"], "verified", "{title}");
    assert!(array(&title["comparisons"]).iter().all(|c| c["category"] == "typography" || c["category"] == "color"));
    // The failed default plus the hover and disabled variants nobody checked.
    let items = array(&m["followUp"]["fixInScope"]).iter().map(|f| text(&f["item"]).to_owned()).collect::<Vec<_>>();
    assert_eq!(items, ["state Button default", "state Button disabled", "state Button hover"]);
}

#[tokio::test]
async fn a_state_without_design_or_capture_is_unverified() {
    let run = Run::new();
    let m = run.matrix(json!({"stateChecks":[
        {"target":"Card","state":"selected","implementation":{"width":10},"implementationSource":"browser"},
        {"target":"Card","state":"focus","design":{"width":10}},
        {"target":"Card","state":"default","design":{"width":10,"height":5},"implementation":{"width":10},"implementationSource":"browser"}
    ]})).await;
    assert!(text(&state(&m, "Card", "selected")["reason"]).starts_with("No design"));
    assert!(text(&state(&m, "Card", "focus")["reason"]).starts_with("No capture"));
    let partial = state(&m, "Card", "default");
    assert_eq!(partial["verdict"], "unverified");
    assert_eq!(partial["notMeasured"], json!(["height"]));
}

#[tokio::test]
async fn visual_comparisons_reuse_devup_visual_compare() {
    let mut run = Run::new();
    std::fs::write(run.dir.0.join("ref.png"), STANDARD.decode(WHITE).unwrap()).unwrap();
    std::fs::write(run.dir.0.join("same.png"), STANDARD.decode(WHITE).unwrap()).unwrap();
    std::fs::write(run.dir.0.join("other.png"), STANDARD.decode(BLACK).unwrap()).unwrap();
    let reference = sha2::Sha256::digest(STANDARD.decode(WHITE).unwrap()).iter().map(|b| format!("{b:02x}")).collect::<String>();
    let environment = json!({"schemaVersion":1,"tsxResource":{"uri":"devup://artifact/test/manifest","sha256":"a".repeat(64)},
        "referenceResource":{"uri":"file://ref.png","sha256":reference},"viewport":{"width":1,"height":1,"deviceScaleFactor":1},
        "captureContext":{"innerWidth":1,"innerHeight":1,"devicePixelRatio":1,"visualViewportScale":1,"rootCssZoom":1,"bodyCssZoom":1},
        "themePath":"devup.json","assetDirectory":"public","fontManifest":[{"family":"Test","sha256":"b".repeat(64)}],
        "renderer":{"name":"playwright-chromium","version":"1"},"devupUiVersion":"1","actualPng":"other.png",
        "environmentStatus":"valid","os":"test","locale":"en-US","timezone":"UTC"});
    let checks = json!({"stateChecks":[
        {"target":"Hero","state":"default","visual":{"actual":"other.png","reference":{"path":"ref.png"},"diffPath":"diff.png","environment":environment}},
        {"target":"Hero","state":"hover","visual":{"actual":"same.png","reference":{"path":"ref.png"}}}
    ]});
    let without_policy = run.matrix(checks.clone()).await;
    assert_eq!(state(&without_policy, "Hero", "default")["visual"]["verdict"], "error");
    assert_eq!(state(&without_policy, "Hero", "default")["verdict"], "unverified");
    run.policy = Some(OutputPolicy::from_roots(vec![run.dir.0.clone()]).unwrap());
    let m = run.matrix(checks).await;
    let failed = state(&m, "Hero", "default");
    assert_eq!(failed["verdict"], "failed", "{failed}");
    assert_eq!(failed["visual"]["tool"], "devup_visual_compare");
    assert!(failed["capture"]["diffPath"].as_str().unwrap().ends_with("diff.png"));
    assert_eq!(failed["capture"]["needed"], false);
    // A page comparison without an environment manifest is inconclusive.
    assert_eq!(state(&m, "Hero", "hover")["verdict"], "unverified");
}

#[tokio::test]
async fn requirements_link_scope_and_demand_observed_evidence() {
    let run = Run::new();
    let m = run.matrix(json!({"requirements":[
        {"id":"R1","text":"Button shows hover colour","origin":"design-spec-card","figmaNodeId":"2:1","check":"interaction",
            "targets":[{"componentPath":"./components/Button.tsx"}],
            "evidence":[{"source":"static","status":"pass","detail":"_hover prop present"}]},
        {"id":"R2","text":"Print layout","notApplicableReason":"No print stylesheet in scope","targets":[{"routePath":"/a"}]},
        {"id":"R3","text":"Floating prose"},
        {"id":"R4","text":"Button label is bold","check":"content","targets":[{"componentPath":"components/Button.tsx"}],
            "evidence":[{"source":"browser","status":"pass","detail":"bold on /a","routePath":"/a"}]},
        {"id":"R5","text":"Hero copy","check":"content","targets":[{"componentPath":"components/Hero.tsx"}],
            "evidence":[{"source":"automated-test","status":"pass","detail":"hero.test.tsx"}]}
    ]})).await;
    let find = |id: &str| array(&m["requirements"]).iter().find(|r| r["id"] == id).unwrap().clone();
    let r1 = find("R1");
    assert_eq!(r1["verdict"], "unverified");
    assert_eq!(r1["outcome"], "browser-verification-required");
    assert_eq!(r1["scope"]["screens"], json!(["/a", "/b"]));
    assert_eq!(r1["scope"]["multiScreen"], true);
    assert_eq!(r1["scope"]["sharedComponents"][0]["component"], "components/Button.tsx");
    assert_eq!(find("R2")["verdict"], "not-applicable");
    assert_eq!(find("R3")["outcome"], "unlinked");
    let r4 = find("R4");
    assert_eq!(r4["outcome"], "scope-not-covered");
    assert_eq!(r4["scope"]["uncoveredScreens"], json!(["/b"]));
    assert_eq!(find("R5")["verdict"], "verified");
    let impact = array(&m["changeImpact"]);
    let scope = |path: &str| impact.iter().find(|i| i["path"] == path).unwrap()["scope"].clone();
    assert_eq!(scope("components/Button.tsx"), "shared");
    assert_eq!(scope("components/Hero.tsx"), "screen-only");
}

#[tokio::test]
async fn requirements_follow_their_linked_states() {
    let run = Run::new();
    let m = run.matrix(json!({
        "stateChecks":[{"target":"components/Hero.tsx","routePath":"/a","state":"default","design":{"width":10},"implementation":{"width":10},"implementationSource":"browser"}],
        "requirements":[
            {"id":"V1","text":"Hero matches","check":"visual","targets":[{"componentPath":"components/Hero.tsx","state":"default"}]},
            {"id":"V2","text":"Hero hover","check":"visual","targets":[{"componentPath":"components/Hero.tsx","state":"hover"}]}]
    })).await;
    let find = |id: &str| array(&m["requirements"]).iter().find(|r| r["id"] == id).unwrap().clone();
    assert_eq!(find("V1")["verdict"], "verified", "{m}");
    assert_eq!(find("V2")["verdict"], "unverified");
    assert_eq!(m["claims"]["designMatch"], "allowed");
    assert_eq!(m["claims"]["verificationComplete"], "blocked");
}

#[tokio::test]
async fn incomplete_figma_data_names_gaps_and_what_cannot_be_judged() {
    let mut run = Run::new();
    let mut payload = payload();
    payload.snapshot.nodes.get_mut("2:1").unwrap().field_errors.insert("fills".into(), "fills could not be read".into());
    payload.snapshot.nodes.get_mut("1:1").unwrap().fields.insert("childrenIds".into(), json!(["2:1", "2:9", "9:9"]));
    payload.failures.push(devup_mcp_figma::ScreenFailure {
        node_id: "8:8".into(),
        error_code: devup_mcp_figma::ErrorCode::DevupInvalidInput,
        message: "timed out".into(),
        retryable: true,
    });
    run.payload = Some(payload);
    let m = run.matrix(json!({
        "stateChecks":[{"target":"components/Button.tsx","routePath":"/a","state":"default","figmaNodeId":"2:1","implementation":button_values(),"implementationSource":"browser"}],
        "requirements":[{"id":"S1","text":"Screen matches","check":"visual","figmaNodeId":"1:1","targets":[{"routePath":"/a"}],
            "evidence":[{"source":"browser","status":"pass","detail":"looked right"}]}],
        "designGaps":[{"nodeId":"7:7","kind":"export-failed","cause":"image render refused"}]
    })).await;
    let gaps = array(&m["designData"]["gaps"]);
    assert_eq!(m["designData"]["complete"], false);
    let kinds = gaps.iter().map(|g| (text(&g["nodeId"]), text(&g["kind"]))).collect::<BTreeSet<_>>();
    for expected in [("2:1", "missing-property"), ("9:9", "missing-node"), ("8:8", "read-failed"), ("7:7", "export-failed")] {
        assert!(kinds.contains(&expected), "{expected:?}: {gaps:?}");
    }
    let fills = gaps.iter().find(|g| g["nodeId"] == "2:1").unwrap();
    assert_eq!(fills["cause"], "fills could not be read");
    assert_eq!(fills["cannotJudge"]["screens"], json!(["/a"]));
    assert_eq!(state(&m, "components/Button.tsx", "default")["verdict"], "unverified");
    let s1 = &m["requirements"][0];
    assert_eq!(s1["outcome"], "design-data-incomplete", "{s1}");
    assert_eq!(m["claims"]["designMatch"], "blocked");
    assert!(array(&m["followUp"]["fixInScope"]).iter().any(|f| text(&f["nextAction"]).contains("devup_figma_export")));
}

#[tokio::test]
async fn an_unreadable_artifact_blocks_every_design_comparison() {
    let mut run = Run::new();
    run.payload = None;
    run.unavailable = Some("artifactId is missing or expired".into());
    let m = run.matrix(json!({"stateChecks":[{"target":"Button","state":"default","figmaNodeId":"2:1","design":{"width":1},"implementation":{"width":1},"implementationSource":"browser"}]})).await;
    assert_eq!(state(&m, "Button", "default")["verdict"], "unverified");
    assert_eq!(m["designData"]["gaps"][0]["nodeId"], "*");
}

#[tokio::test]
async fn review_report_gives_path_steps_expectation_and_separate_results() {
    let run = Run::new();
    let m = run.matrix(json!({
        "stateChecks":[{"target":"components/Button.tsx","routePath":"/a","state":"hover","design":{"width":1},"implementation":{"width":1},"implementationSource":"browser"}],
        "requirements":[{"id":"R1","text":"Hover darkens the button","check":"interaction",
            "targets":[{"routePath":"/a","componentPath":"components/Button.tsx","state":"hover"}],
            "evidence":[{"source":"automated-test","status":"pass","detail":"button.spec","routePath":"/a"},
                {"source":"manual","status":"pass","detail":"checked by hand","routePath":"/b"}]}],
        "changes":[{"summary":"Button hover colour","requirementIds":["R1"],"files":["components/Button.tsx"]}]
    })).await;
    let change = &m["reviewReport"]["changes"][0];
    assert_eq!(change["path"], "/a");
    assert_eq!(change["steps"], json!(["Open /a", "Move the pointer over components/Button.tsx"]));
    assert_eq!(change["expected"], "Hover darkens the button");
    assert_eq!(array(&change["results"]["automated"]).len(), 1);
    assert_eq!(array(&change["results"]["manual"]).len(), 1);
    assert_eq!(array(&change["results"]["browser"]).len(), 1);
    assert_eq!(change["captures"], json!([]));
    assert_eq!(change["verdict"], "verified", "{m}");
    assert_eq!(m["resultsBySource"]["static"], 0);
}

#[tokio::test]
async fn follow_up_orders_fixes_questions_and_unresolved_items() {
    let run = Run::new();
    let m = run.matrix(json!({"requirements":[
        {"id":"P","text":"Login flow","check":"interaction","targets":[{"routePath":"/a"}],
            "blocker":{"kind":"permission","cause":"Staging needs a test account","recommendation":"Provide a test account"}},
        {"id":"U","text":"Safari rendering","check":"visual","targets":[{"routePath":"/a"}],"attempts":["Tried WebKit in Playwright"],
            "blocker":{"kind":"unresolvable","cause":"No macOS runner available"}},
        {"id":"F","text":"Fails","check":"static","targets":[{"routePath":"/a"}],"evidence":[{"source":"static","status":"fail","detail":"lint error"}]}
    ]})).await;
    let follow = &m["followUp"];
    assert_eq!(array(&follow["order"]).len(), 5);
    assert_eq!(follow["askUser"][0]["incompleteRequest"], json!(["verifies", "userAction"]));
    assert_eq!(follow["continueWhileWaiting"], true);
    assert_eq!(follow["unresolved"][0]["attempts"], json!(["Tried WebKit in Playwright"]));
    assert_eq!(follow["unresolved"][0]["remainingConstraint"], "No macOS runner available");
    assert!(text(&follow["fixInScope"][0]["nextAction"]).starts_with("Fix the implementation"));
}

#[tokio::test]
async fn config_scope_separates_shared_files_from_screen_files() {
    let run = Run::new();
    std::fs::write(run.dir.0.join("devup.json"), "{}").unwrap();
    let m = run.matrix(json!({"changes":[{"summary":"x","files":["components/Unknown.tsx"]}]})).await;
    assert_eq!(m["configScope"]["shared"][0]["path"], "devup.json");
    assert_eq!(array(&m["configScope"]["screenOnly"]).len(), 2);
    assert_eq!(m["changeImpact"][0]["scope"], "unknown");
    assert_eq!(m["reviewReport"]["changes"][0]["missingReviewPath"], true);
}

#[test]
fn colours_and_states_normalise() {
    assert_eq!(parse_color("#f00"), Some([255, 0, 0, 255]));
    assert_eq!(parse_color("rgba(255, 0, 0, 0.5)"), Some([255, 0, 0, 128]));
    assert_eq!(parse_color("rgb(255 0 0 / 50%)"), Some([255, 0, 0, 128]));
    assert_eq!(normalize_state("Pressed"), "active");
    assert_eq!(normalize_state("focus-visible"), "focus");
    assert_eq!(normalize_state("Checked"), "selected");
    assert_eq!(font_weight_from_style("Semi Bold"), Some(600.0));
}

#[tokio::test]
async fn a_figma_state_target_without_a_state_check_is_not_linked() {
    let run = Run::new();
    let m = run.matrix(json!({
        "stateChecks":[{"target":"Other","state":"hover","figmaNodeId":"2:9","design":{"width":1},"implementation":{"width":1},"implementationSource":"browser"}],
        "requirements":[{"id":"N","text":"Button hover","check":"static","targets":[{"figmaNodeId":"2:1","state":"hover"}],
            "evidence":[{"source":"static","status":"pass","detail":"prop present"}]}]
    })).await;
    let requirement = &m["requirements"][0];
    assert_eq!(requirement["outcome"], "missing-evidence", "{requirement}");
    assert_eq!(requirement["linkedStates"], json!([]));
}
