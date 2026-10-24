//! Feedback 002/005: executable evidence for interaction and context boundaries.
use super::*;
use devup_mcp_figma::Snapshot;

struct Project(std::path::PathBuf);
impl Project {
    fn new() -> Self {
        let p = Self(std::env::temp_dir().join(format!(
            "devup-feedback-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        )));
        p.write("package.json", "{}");
        p
    }
    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn root(&self) -> &str {
        self.0.to_str().unwrap()
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn design() -> Snapshot {
    let nodes = json!([
        {"id":"screen","type":"FRAME","fields":{"name":"Main","childrenIds":["header1","checkbox","radio","card","widget","chip","timer"]}},
        {"id":"other","type":"FRAME","fields":{"name":"Inquiry","childrenIds":["header2"]}},
        {"id":"header1","type":"INSTANCE","fields":{"name":"GNB","mainComponentId":"header-master","componentProperties":{"State":{"value":"active"}},"childrenIds":["nav","privacy"]}},
        {"id":"header2","type":"INSTANCE","fields":{"name":"GNB","mainComponentId":"header-master","componentProperties":{"State":{"value":"disabled"}},"childrenIds":["nav2"]}},
        {"id":"nav","type":"TEXT","fields":{"name":"Menu item","characters":"포트폴리오","reactions":[{"trigger":{"type":"ON_CLICK"},"actions":[{"type":"NODE","navigation":"NAVIGATE","destinationId":"portfolio-screen"}]}]}},
        {"id":"nav2","type":"TEXT","fields":{"name":"Label","characters":"고객센터"}},
        {"id":"privacy","type":"TEXT","fields":{"name":"Label","characters":"개인정보 처리방침","reactions":[{"trigger":{"type":"ON_CLICK"},"action":{"type":"URL","url":"https://example.com/privacy"}}]}},
        {"id":"checkbox","type":"INSTANCE","fields":{"name":"Checkbox round","variantProperties":{"Selected":"True","Disabled":"False"},"cornerRadius":20,"strokes":[{"type":"SOLID"}]}},
        {"id":"radio","type":"INSTANCE","fields":{"name":"Radio","variantProperties":{"Selected":"False"}}},
        {"id":"card","type":"INSTANCE","fields":{"name":"PortfolioCard","reactions":[{"trigger":{"type":"ON_CLICK"},"actions":[{"type":"NODE","destinationId":"detail"}]}]}},
        {"id":"widget","type":"FRAME","fields":{"name":"reCAPTCHA widget"}},
        {"id":"chip","type":"INSTANCE","fields":{"name":"Chip chevron outline","cornerRadius":12,"paddingLeft":8,"paddingRight":8,"strokeWeight":1}},
        {"id":"timer","type":"FRAME","fields":{"name":"Animation","reactions":[{"trigger":{"type":"AFTER_TIMEOUT"},"actions":[{"type":"NODE","destinationId":"other"}]}]}}
    ]);
    let nodes = nodes
        .as_array()
        .unwrap()
        .iter()
        .map(|v| serde_json::from_value(v.clone()).unwrap())
        .map(|n: devup_mcp_figma::RawNode| (n.id.clone(), n))
        .collect();
    Snapshot {
        file_key: "fixture".into(),
        version: None,
        roots: vec!["screen".into(), "other".into()],
        nodes,
        diagnostics: vec![],
    }
}

#[test]
fn feedback_interactions_preserve_targets_roles_states_and_unresolved_data() {
    let schema = Value::Object((*interactions::output_schema()).clone());
    assert_eq!(
        schema["properties"]["interactiveCandidates"]["items"]["$ref"],
        "#/$defs/InteractiveCandidate"
    );
    assert_eq!(
        schema["$defs"]["Role"]["enum"],
        json!(["link", "button", "tab", "nav", "checkbox", "radio"])
    );
    assert_eq!(
        schema["$defs"]["InteractiveCandidate"]["properties"]["targetResolved"]["type"],
        "boolean"
    );
    let snapshot = design();
    let evidence = interactions::evidence(&snapshot, &snapshot.roots);
    let candidates = evidence["interactiveCandidates"].as_array().unwrap();
    let find = |id: &str| candidates.iter().find(|c| c["nodeId"] == id).unwrap();
    assert_eq!(find("checkbox")["role"], "checkbox");
    assert_eq!(find("radio")["role"], "radio");
    assert_eq!(
        find("checkbox")["states"]["variantProperties"]["Selected"],
        "True"
    );
    assert_eq!(
        find("nav")["prototypeActions"][0]["destinationId"],
        "portfolio-screen"
    );
    assert_eq!(
        find("privacy")["prototypeActions"][0]["url"],
        "https://example.com/privacy"
    );
    assert_eq!(find("card")["hasPrototypeTarget"], true);
    assert_eq!(find("card")["targetResolved"], false);
    assert!(
        find("card")["unresolvedReasons"]
            .as_array()
            .unwrap()
            .contains(&json!("record-id-data-not-verified"))
    );
    assert!(!candidates.iter().any(|c| c["nodeId"] == "timer"));
    assert!(candidates.iter().all(|c| c["targetResolved"] == false));
    let mut snapshot = design();
    let node = snapshot.nodes.get_mut("radio").unwrap();
    node.fields.insert("name".into(), json!("Tab"));
    let node = snapshot.nodes.get_mut("card").unwrap();
    node.fields.insert("reactions".into(),json!([{"trigger":{"type":"ON_CLICK"},"actions":[{"type":"NODE","navigation":"OVERLAY","destinationId":"dialog"}]}]));
    let candidates =
        serde_json::to_value(interactions::candidates(&snapshot, &snapshot.roots)).unwrap();
    assert!(
        candidates
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["nodeId"] == "radio" && c["role"] == "tab")
    );
    assert!(
        candidates
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["nodeId"] == "card"
                && c["prototypeActions"][0]["navigation"] == "OVERLAY"
                && c["targetResolved"] == false)
    );
}

#[test]
fn feedback_shared_widgets_details_and_grouped_questions_require_review() {
    let snapshot = design();
    let mut result = json!({"status":"complete","quality":{"projection":"exact"},"tsx":"<Box />","deliverable":{"isFinal":true}});
    interactions::attach(&mut result, &snapshot);
    review::attach(&mut result);
    assert_eq!(
        result["sharedComponentCandidates"][0]["variantDifferences"],
        true
    );
    assert_eq!(result["widgetCandidates"][0]["nodeId"], "widget");
    assert!(
        result["stateAndIconDetails"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["iconStyle"] == "outline-name-hint" && d["fields"]["paddingLeft"] == 8)
    );
    assert_eq!(result["ambiguityQuestions"].as_array().unwrap().len(), 3);
    assert!(
        result["reviewChecklist"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["code"] == "DEVUP_CODEGEN_INTERACTION_UNMAPPED"
                && i["severity"] == "required")
    );
    assert_eq!(result["deliverable"]["isFinal"], false);
    assert_eq!(
        result["completionEvidence"]["visualCompare"]["automaticRun"],
        false
    );
    assert_eq!(result["interactionCoverage"]["wiredCount"], 0);
    // No master identity: same name is insufficient evidence.
    let mut old = snapshot;
    for node in old.nodes.values_mut() {
        node.fields.remove("mainComponentId");
    }
    assert!(
        interactions::evidence(&old, &old.roots)["sharedComponentCandidates"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn feedback44_export_wiring_guidance_keeps_states_on_the_anchor() {
    let mut result = json!({"tsx":"<Box />","deliverable":{"isFinal":true}});
    interactions::attach(&mut result, &design());
    let issue = result["projectionIssues"].as_array().unwrap().iter()
        .find(|issue| issue["code"] == "DEVUP_CODEGEN_INTERACTION_UNMAPPED").unwrap();
    let guidance = issue["details"]["nextAction"].as_str().unwrap();
    assert!(guidance.contains("as={Link} props={{ href, 'aria-current'"));
    for state in ["_hover={hoverStyles}", "_focusVisible={focusStyles}", "_active={activeStyles}"] {
        assert!(guidance.contains(state), "{guidance}");
    }
    assert!(guidance.contains("not on a child wrapped in Link"));
    assert_eq!(result["interactionCoverage"]["wiredCount"], 0);
    assert_eq!(result["deliverable"]["isFinal"], false);
}

#[tokio::test]
async fn feedback_context_routes_reuse_env_names_and_no_secret_values() {
    let p = Project::new();
    p.write(
        "src/app/(public)/portfolio/[id]/page.tsx",
        "export default function Page(){return <main/>}",
    );
    p.write(
        "src/app/privacy/page.tsx",
        "export default function Page(){return <main/>}",
    );
    p.write(
        "src/components/Header.tsx",
        "export function Header(){return <header/>}",
    );
    p.write(".env.local","NEXT_PUBLIC_RECAPTCHA_SITE_KEY=secret-site-value\nRECAPTCHA_SECRET_KEY=secret-server-value\nexport OTHER_KEY='super-private'\n# BAD=ignore\n");
    p.write(".worktrees/stale/.git", "gitdir: another");
    p.write(".worktrees/stale/.env", "EXCLUDED_KEY=private");
    let routes = project_context::run("routes", Some(p.root()), None)
        .await
        .unwrap();
    assert!(routes.get("components").is_none());
    assert!(
        routes["routes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["route"] == "/portfolio/[id]")
    );
    let context = project_context::export_context(p.root());
    assert!(
        context["ui"]["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["path"] == "src/components/Header.tsx")
    );
    assert_eq!(
        context["envKeyNames"],
        json!([
            "NEXT_PUBLIC_RECAPTCHA_SITE_KEY",
            "OTHER_KEY",
            "RECAPTCHA_SECRET_KEY"
        ])
    );
    let serialized = context.to_string();
    for value in [
        "secret-site-value",
        "secret-server-value",
        "super-private",
        "EXCLUDED_KEY",
    ] {
        assert!(!serialized.contains(value));
    }
    let mut result = json!({"tsx":"<Box/>","interactionCoverage":{},"interactiveCandidates":[{"nodeId":"a","text":"포트폴리오"},{"nodeId":"b","text":"소개"}]});
    result = with_project_checks(
        result,
        Some(p.root()),
        &skills::Lookup::project_only(p.0.clone(), skills::Runtime::Codex),
    );
    assert_eq!(
        result["routeSuggestions"][0]["suggestions"][0]["route"],
        "/portfolio/[id]"
    );
    assert_eq!(
        result["routeSuggestions"][0]["suggestions"][0]["requiresRecordId"],
        true
    );
    assert_eq!(result["routeSuggestions"][1]["suggestions"], json!([]));
    assert_eq!(result["routeSuggestions"][0]["targetResolved"], false);
    assert!(!result.to_string().contains("secret-site-value"));
}

#[test]
fn feedback_validation_catches_missing_routes_unmapped_actions_and_active_state() {
    let inventory = json!({"found":true,"routes":[{"route":"/portfolio/[id]","appliesTo":"."},{"route":"/privacy","appliesTo":"."}]});
    let source = r#"import NavLink from 'next/link'; export function Header(){return <><NavLink href='/inquiry'>견적문의</NavLink><Text>전체 보기</Text><Text onClick={()=>{}}>동의</Text><NavLink href='/privacy' aria-current='page'><Text>개인정보 처리방침</Text></NavLink><NavLink href={'/portfolio/real-id'}>Card</NavLink><button>견적문의</button><button type='button' onClick={()=>{}}>Open</button><input type='checkbox'/></>}"#;
    let report = navigation::report(
        source,
        &inventory,
        Some(
            "<!DOCTYPE html><html><body><a href='/privacy'>A</a><a href='/privacy'>B</a><a href='/portfolio/real-id?x=1&amp;y=2'>C</a></body></html>",
        ),
    );
    let findings = report["findings"].as_array().unwrap();
    assert_eq!(
        findings
            .iter()
            .filter(|f| f["rule"] == "missing-static-route")
            .count(),
        1
    );
    assert!(findings.iter().any(|f| f["rule"] == "missing-static-route"
        && f["severity"] == "error"
        && f["href"] == "/inquiry"));
    assert_eq!(
        findings
            .iter()
            .filter(|f| f["rule"] == "interaction-unmapped")
            .count(),
        3
    );
    assert!(
        findings
            .iter()
            .any(|f| f["rule"] == "interaction-accessibility")
    );
    assert!(
        findings
            .iter()
            .any(|f| f["rule"] == "hardcoded-active-state" && f["severity"] == "info")
    );
    assert_eq!(report["sourceHrefs"]["/privacy"], 1);
    assert_eq!(report["renderedHrefs"]["counts"]["/privacy"], 2);
    assert_eq!(
        report["renderedHrefs"]["counts"]["/portfolio/real-id?x=1&y=2"],
        1
    );
    assert_eq!(report["coverage"]["runtimeVerified"], false);
}

#[test]
fn a_primitive_rendered_as_link_with_props_href_is_wired() {
    let inventory = json!({"found":true,"routes":[{"route":"/company","appliesTo":"."}]});
    let source = r#"import Link from 'next/link'; export function Header(){return <><Text as={Link} props={{ href: '/company' }}>회사소개</Text><Text as={Link} props={{ href: '/missing' }}>없음</Text><Text>전체 보기</Text></>}"#;
    let report = navigation::report(source, &inventory, None);
    let findings = report["findings"].as_array().unwrap();
    let unmapped = findings
        .iter()
        .filter(|f| f["rule"] == "interaction-unmapped")
        .count();
    assert_eq!(unmapped, 1, "{findings:?}");
    assert!(findings.iter().any(|f| f["rule"] == "missing-static-route" && f["href"] == "/missing"));
    assert_eq!(report["sourceHrefs"]["/company"], 1);
    let dynamic = r#"import Link from 'next/link'; const Menu=({items})=> <>{items.map((item)=> <Box as={Link} props={{ href: item.href }}>{item.label}</Box>)}</></>"#;
    let report = navigation::report(dynamic, &inventory, None);
    assert!(
        !report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["rule"] == "interaction-unmapped"),
        "{report}"
    );
}

#[test]
fn feedback_fixed_navigation_underline_is_an_unverified_info_candidate() {
    let source = r#"import Link from 'next/link'; const Header=()=> <nav><Link href='/services' style={{borderBottom:'3px solid red'}}>서비스</Link><Link href='/privacy' borderBottomWidth={3} borderBottomColor='$primary'>Privacy</Link><Link href='/terms' style={{borderBottom: active ? '3px solid red' : 'none'}}>Terms</Link><Link href='/about' style={{...props,borderBottom:'3px solid red'}}>About</Link><Link href='/selected' aria-current='page' borderBottom='3px solid red'>Selected</Link></nav>"#;
    let report = navigation::report(source, &json!({}), None);
    let findings = report["findings"].as_array().unwrap();
    assert_eq!(
        findings
            .iter()
            .filter(|f| f["rule"] == "fixed-navigation-marker" && f["severity"] == "info")
            .count(),
        2
    );
    assert_eq!(
        findings
            .iter()
            .filter(|f| f["rule"] == "hardcoded-active-state")
            .count(),
        1
    );
    assert_eq!(report["coverage"]["runtimeVerified"], false);
    assert!(
        report["elements"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["fixedVisualNavigationMarker"] == true)
            .all(|e| e["staticActive"] == false)
    );
    let decorative = navigation::report(
        "const Card=()=> <Box style={{borderBottom:'3px solid red'}}>Decorative heading</Box>",
        &json!({}),
        None,
    );
    assert!(decorative["findings"].as_array().unwrap().is_empty());
}

#[test]
fn feedback_prime_link_center_text_marker_respects_border_order_and_dynamic_pathname() {
    let source = r#"import Link from 'next/link'; const Header=()=> <nav><Link href='/services'><Center borderBottom='solid 3px $primary'><Text>서비스</Text></Center></Link><Link href='/privacy'><Center borderBottom={pathname === '/privacy' ? 'solid 3px $primary' : 'none'}><Text>Privacy</Text></Center></Link><Center borderBottom='solid 3px $primary'><Text>Decorative</Text></Center></nav>"#;
    let report = navigation::report(source, &json!({}), None);
    let markers = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["rule"] == "fixed-navigation-marker")
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0]["element"], "Center");
    assert_eq!(markers[0]["severity"], "info");
    assert_eq!(report["coverage"]["candidateCount"], 2);
    assert_eq!(report["coverage"]["runtimeVerified"], false);
    assert!(
        !report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["rule"] == "hardcoded-active-state")
    );
}

#[test]
fn feedback_opaque_prime_captcha_image_is_only_a_review_candidate() {
    let mut data = design();
    let root = data.nodes.get_mut("widget").unwrap();
    root.node_type = "RECTANGLE".into();
    root.fields.clear();
    root.fields.insert("name".into(), json!("image 1"));
    root.fields.insert("width".into(), json!(304));
    root.fields.insert("height".into(), json!(78));
    root.fields.insert(
        "fills".into(),
        json!([{"type":"IMAGE","imageHash":"captured-image-hash"}]),
    );
    let evidence = interactions::evidence(&data, &["widget".into()]);
    assert!(
        evidence["widgetCandidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["nodeId"] == "widget"
                && w["verified"] == false
                && w["reason"] == "captcha-sized-opaque-image-provider-and-content-unverified")
    );
    data.roots = vec!["widget".into()];
    let mut result = json!({"tsx":"const View=()=> <Box/>;","deliverable":{"isFinal":true}});
    interactions::attach(&mut result, &data);
    assert_eq!(result["deliverable"]["isFinal"], false);
    assert_eq!(result["interactionCoverage"]["candidateCount"], 0);
    for paint in [
        json!({"type":"SOLID"}),
        json!({"type":"IMAGE","imageHash":"source","visible":false}),
        json!({"type":"IMAGE","imageHash":"source","opacity":0.0}),
    ] {
        data.nodes
            .get_mut("widget")
            .unwrap()
            .fields
            .insert("fills".into(), json!([paint]));
        assert!(
            interactions::evidence(&data, &["widget".into()])["widgetCandidates"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn feedback_validation_limits_do_not_claim_absence_or_success() {
    let source = r#"import Link from 'next/link'; const Screen=()=> <><Link href={destination}>Dynamic</Link><a href='https://example.com'>External</a><a href='#section'>Hash</a><a href='/logo.svg'>Asset</a><Text>Database table</Text></>"#;
    for inventory in [
        json!({}),
        json!({"found":true,"truncation":{"truncated":true}}),
        json!({"found":true,"unparsedFiles":1}),
        json!({"found":true,"customRoutingUnverified":true}),
        json!({"found":true,"routes":[{"route":"/","appliesTo":"apps/a"},{"route":"/other","appliesTo":"apps/b"}]}),
    ] {
        let report = navigation::report(source, &inventory, None);
        assert_eq!(report["routeCheck"]["ran"], false);
        assert!(
            report["findings"].as_array().unwrap().is_empty(),
            "{report}"
        );
        assert_eq!(report["coverage"]["dynamicTargetCount"], 1);
    }
    assert_eq!(navigation::source_evidence("<Broken")["parsed"], false);
    assert!(!navigation::matches_route(
        "/portfolio/[id]",
        "/portfolio/[id]"
    ));
    assert!(navigation::matches_route("/docs/[...slug]", "/docs/a/b"));
    assert!(!navigation::matches_route("/docs/[...slug]", "/docs"));
    assert!(navigation::matches_route("/docs/[[...slug]]", "/docs"));
}

#[test]
fn feedback_validation_merged_verdict_and_bundle_disk_independence() {
    let p = Project::new();
    p.write(
        "app/privacy/page.tsx",
        "export default function Page(){return <main/>}",
    );
    let input=serde_json::from_value(json!({"tsx":"import Link from 'next/link'; const C=()=> <Link href='/inquiry'>견적문의</Link>","projectRoot":p.root()})).unwrap();
    let mut result = json!({"ok":true,"okReason":"clean","violations":[]});
    navigation::attach_validation(&mut result, &input);
    assert_eq!(result["ok"], false);
    assert_eq!(result["okReason"], "error-violations");
    assert_eq!(result["violationCounts"]["error"], 1);
    let input=serde_json::from_value(json!({"files":[{"path":"app/privacy/page.tsx","content":"export default function Page(){return <Text>전체 보기</Text>}"}],"projectRoot":"Z:/must-never-be-read","strict":true})).unwrap();
    let mut result = json!({"ok":true,"okReason":"clean","violations":[]});
    navigation::attach_validation(&mut result, &input);
    assert_eq!(result["ok"], false);
    assert_eq!(result["okReason"], "strict-warnings");
    assert_eq!(
        result["navigation"]["sources"][0]["report"]["routeCheck"]["ran"],
        false
    );
    assert!(result["navigation"]["routeInventory"]["projectRoot"].is_null());
}

#[test]
fn feedback_navigation_graph_compares_explicit_targets_without_inference() {
    let candidates =
        serde_json::to_value(interactions::candidates(&design(), &["screen".into()])).unwrap();
    let source = r#"import Link from 'next/link'; const C=()=> <><Link href='/wrong' data-figma-node-id='nav'>Portfolio</Link><a href='https://example.com/privacy'>Privacy</a></>"#;
    let graph = navigation::graph(
        &candidates,
        &[("Header.tsx".into(), source.into())],
        &std::collections::BTreeMap::from([("portfolio-screen".into(), "/portfolio".into())]),
    );
    let comparison = graph["comparison"].as_array().unwrap();
    assert!(
        comparison
            .iter()
            .any(|c| c["nodeId"] == "nav" && c["status"] == "MISMATCH")
    );
    assert!(
        comparison
            .iter()
            .any(|c| c["nodeId"] == "privacy" && c["status"] == "MATCHED_STATIC_HREF")
    );
    assert!(comparison.iter().any(|c| c["nodeId"] == "card"
        && c["status"] == "UNVERIFIED"
        && c["expectedTarget"].is_null()));
    let graph = navigation::graph(&candidates, &[], &Default::default());
    assert!(
        graph["comparison"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["status"] == "MISSING_STATIC_HREF")
    );
}

#[test]
fn feedback_widget_compositions_and_large_metadata_are_bounded() {
    let mut snapshot = design();
    for (id, name) in [("map", "Google map widget"), ("payment", "Stripe payment")] {
        let node: devup_mcp_figma::RawNode =
            serde_json::from_value(json!({"id":id,"type":"FRAME","fields":{"name":name}})).unwrap();
        snapshot.nodes.insert(id.into(), node);
        snapshot.roots.push(id.into());
    }
    for value in [
        json!({"id":"panel","type":"FRAME","fields":{"name":"Frame 123","width":304,"height":78,"childrenIds":["square","label","mark"]}}),
        json!({"id":"square","type":"RECTANGLE","fields":{"width":28,"height":28}}),
        json!({"id":"label","type":"TEXT","fields":{"characters":"I'm not a robot"}}),
        json!({"id":"mark","type":"VECTOR","fields":{}}),
    ] {
        let node: devup_mcp_figma::RawNode = serde_json::from_value(value).unwrap();
        snapshot.nodes.insert(node.id.clone(), node);
    }
    snapshot.roots.push("panel".into());
    let evidence = interactions::evidence(&snapshot, &snapshot.roots);
    for id in ["map", "payment"] {
        assert!(
            evidence["widgetCandidates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w["nodeId"] == id && w["verified"] == false)
        );
    }
    assert!(
        evidence["widgetCandidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["nodeId"] == "panel"
                && w["reason"] == "captcha-sized-panel-square-control-text-and-mark-composition"
                && w["verified"] == false)
    );
    for i in 0..250 {
        let id = format!("button-{i}");
        let node:devup_mcp_figma::RawNode=serde_json::from_value(json!({"id":id,"type":"INSTANCE","fields":{"name":"CTA Button","componentProperties":{"State":{"value":"selected"},"Text":{"value":"x".repeat(10_000)}}}})).unwrap();
        snapshot.nodes.insert(id.clone(), node);
        snapshot.roots.push(id);
    }
    let evidence = interactions::evidence(&snapshot, &snapshot.roots);
    assert!(evidence.to_string().len() <= 65_536);
    assert_eq!(evidence["interactionEvidenceLimits"]["truncated"], true);
    assert!(
        evidence["interactionCoverage"]["candidateCount"]
            .as_u64()
            .unwrap()
            > evidence["interactionCoverage"]["returnedCandidateCount"]
                .as_u64()
                .unwrap()
    );
    let mut result = json!({"tsx":"<Box/>","deliverable":{"isFinal":true}});
    interactions::attach(&mut result, &snapshot);
    review::attach(&mut result);
    assert_eq!(result["deliverable"]["isFinal"], false);
    assert!(
        result["reviewChecklist"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["code"] == "DEVUP_CODEGEN_INTERACTION_TRUNCATED"
                && i["severity"] == "required")
    );
}

#[test]
fn feedback_native_semantics_hrefs_and_html_do_not_overstate_wiring() {
    let source = r#"import Link from 'next/link'; const C=()=> <><button>견적문의</button><button onClick={()=>{}}>Open</button><input type='checkbox'/><Link href={target}>Dynamic</Link><a href='#'>Placeholder</a><a href='/portfolio/real?x=1#card'>Card</a><a href='mailto:hello@example.com'>Mail</a><form action='/submit'><input name='email'/><button type='submit'>Submit</button></form><Text>견적문의 화면에 대한 설명이며 클릭 대상이 아닙니다.</Text></>"#;
    let inventory = json!({"found":true,"routes":[{"route":"/portfolio/[id]","appliesTo":"."}]});
    let html = r#"<html><script>const template="<a href='/fake'>";</script><!-- <a href='/comment'> --><body><A href=/privacy>Privacy</A></body></html>"#;
    let report = navigation::report(source, &inventory, Some(html));
    assert_eq!(report["coverage"]["nativeToggleOnlyCount"], 1);
    assert_eq!(report["coverage"]["dynamicTargetCount"], 1);
    assert_eq!(report["coverage"]["wiredCount"], 5);
    assert!(
        !report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["rule"] == "missing-static-route")
    );
    assert_eq!(report["renderedHrefs"]["counts"], json!({"/privacy":1}));
    assert_eq!(report["renderedHrefs"]["parsed"], true);
}

#[tokio::test]
async fn feedback_route_handlers_public_files_and_custom_routing_avoid_false_errors() {
    let p = Project::new();
    p.write(
        "app/page.tsx",
        "export default function Page(){return <main/>}",
    );
    p.write("app/api/contact/route.ts", "export function GET(){}");
    p.write("public/download", "synthetic");
    let source = "const C=()=> <><a href='/api/contact'>API</a><a href='/download'>Download</a></>";
    let inventory = project_context::routes_for_validation(Some(p.root()));
    assert_eq!(
        navigation::report(source, &inventory, None)["findings"],
        json!([])
    );
    p.write(
        "next.config.ts",
        "export default { rewrites: async()=> [] }",
    );
    let inventory = project_context::routes_for_validation(Some(p.root()));
    assert_eq!(
        navigation::report("<a href='/inquiry'>CTA</a>", &inventory, None)["routeCheck"]["ran"],
        false
    );
}

#[tokio::test]
async fn feedback_feature_trace_reads_real_export_hints_and_source_slice() {
    let p = Project::new();
    p.write("app/page.tsx","import Link from 'next/link'; export default function Page(){return <Link href='/portfolio'>Card</Link>}");
    let hint = json!({"nodeId":"card","prototypeActions":[{"destinationId":"portfolio-screen"}],"states":{"selected":true}});
    let input=serde_json::from_value(json!({"projectRoot":p.root(),"routePath":"/","figmaNodeId":"screen","componentTsx":format!("const C=()=> <Box/>;\n// @devup-interaction {hint}"),"navigationTargets":{"portfolio-screen":"/portfolio"}})).unwrap();
    let report = feature_trace::run(input, &artifacts::ArtifactStore::default())
        .await
        .unwrap();
    assert_eq!(
        report["navigationGraph"]["comparison"][0]["status"],
        "MATCHED_STATIC_HREF"
    );
    assert_eq!(
        report["navigationGraph"]["comparison"][0]["expectedTarget"],
        "/portfolio"
    );
    assert_eq!(report["navigationGraph"]["codeEdges"][0]["from"], "/");
}

#[test]
fn feedback_boolean_state_and_form_context_keep_bare_buttons_unresolved() {
    let report = navigation::report(
        "const C=()=> <><button disabled={true}>Disabled</button><form action='/submit'><button type='button'>Local</button><button type='submit'>Submit</button><input type='hidden'/></form><a href='/privacy' aria-selected={true}>Privacy</a></>",
        &json!({}),
        None,
    );
    assert_eq!(report["coverage"]["candidateCount"], 4);
    assert_eq!(report["coverage"]["wiredCount"], 2);
    let findings = report["findings"].as_array().unwrap();
    assert_eq!(
        findings
            .iter()
            .filter(|f| f["rule"] == "interaction-unmapped")
            .count(),
        1
    );
    assert!(
        findings
            .iter()
            .any(|f| f["rule"] == "hardcoded-active-state")
    );
}

#[test]
fn feedback_context_budget_caps_or_defers_explicitly() {
    let p = Project::new();
    p.write(
        "app/page.tsx",
        "export default function Page(){return <main/>}",
    );
    for i in 0..80 {
        p.write(
            &format!("components/Component{i}.tsx"),
            &format!(
                "export function Component{i}(props: {{label: '{}'}}){{return <div/>}}",
                "x".repeat(800)
            ),
        );
    }
    for length in [490_000, 520_000] {
        let mut result = json!({"existingPayload":"x".repeat(length),"interactiveCandidates":[]});
        project_context::attach_export_context(&mut result, p.root());
        assert!(
            serde_json::to_vec(&tool_result(result.clone()))
                .unwrap()
                .len()
                <= delivery::MAX_INLINE_TOTAL_BYTES
        );
        assert!(
            result["projectContext"]["exportContextLimits"]["truncated"] == true
                || result["projectContext"]["state"] == "deferred"
        );
    }
}

#[test]
fn feedback_read_failures_and_direct_selection_preserve_review_and_source_maps() {
    let mut snapshot = design();
    snapshot
        .nodes
        .get_mut("nav")
        .unwrap()
        .field_errors
        .insert("reactions".into(), "field read failed".into());
    let original_map =
        json!({"tsx":[{"elementIndex":0,"nodeId":"nav","generatedProperty":"fontSize"}]});
    let issues=(0..30).map(|i|json!({"code":format!("OLD_{i}"),"fidelityImpact":"lossy","message":"Existing issue"})).collect::<Vec<_>>();
    let mut result = json!({"source":{"nodeId":"screen"},"tsx":"<Box/>","sourceMap":original_map,"projectionIssues":issues,"deliverable":{"isFinal":true}});
    interactions::attach(&mut result, &snapshot);
    review::attach(&mut result);
    assert_eq!(result["sourceMap"], original_map);
    assert!(
        !result["interactiveCandidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["nodeId"] == "nav2")
    );
    assert!(
        result["reviewChecklist"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["code"] == "DEVUP_CODEGEN_INTERACTION_UNVERIFIED"
                && c["severity"] == "required")
    );
    assert_eq!(result["deliverable"]["isFinal"], false);
    assert!(result["summary"]["omittedReviewItems"].as_u64().unwrap() > 0);
}
