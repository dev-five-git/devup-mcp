#[test]
fn status_keeps_failing_links_in_final_report_and_blocks_done() {
    let mut plan = confirmed_plan();
    plan["routes"].as_array_mut().unwrap().truncate(1);
    plan["audit"]["plans"] = json!([{"url":"/"}]);
    let audit = json!({
        "widthTable": ([1440,1100,858,390].map(|width|json!({"width":width}))),
        "screenTable": ([1440,1100,858,390].map(|width|json!({"routePath":"/", "width":width, "status":"pass", "columns":{"completeness":"pass"}})))
    });
    assert_eq!(status_report(&plan,&evidence(),Some(&audit))["workQueue"][0]["done"],true);
    plan["audit"]["linkTable"] = json!([{"from":"http://localhost/", "label":"Missing", "href":"/missing", "status":"fail", "planned":false}]);
    let report = status_report(&plan, &evidence(), Some(&audit));
    assert_eq!(report["linkTable"][0]["href"], "/missing");
    assert!(text(&report["finalReport"]).contains("| from | label | href | status | planned? |"));
    assert!(text(&report["finalReport"]).contains("Missing"));
    assert_eq!(report["workQueue"][0]["done"],false);
}

#[test]
fn a_wide_header_navigation_gets_a_mobile_menu_proposal() {
    let fixture = Fixture::new();
    fixture.write("src/components/Header.tsx", r#"export const Header = () => <Flex as="nav" gap="24px"><Text w="160px">A</Text><Text w="160px">B</Text><Text w="160px">C</Text></Flex>;"#);
    let mut project = evidence();
    project.root = fixture.0.clone();
    project.sources = vec!["src/components/Header.tsx".into()];
    let plan = draft_plan(&main_widths(), &BTreeMap::new(), &project, json!({}), None).unwrap();
    let rows = proposal(array(&plan["questions"]));
    assert!(rows.iter().any(|row| text(&row["topic"]).to_lowercase().contains("mobile menu") && text(&row["recommended"]).to_lowercase().contains("drawer")), "{rows:?}");
}

#[test]
fn measured_exploration_adds_menu_proposal_and_a_menu_frame_suppresses_it() {
    let mut desktop = screen_from_explore(&json!({"nodeId":"desktop", "name":"Home desktop", "bounds":{"width":1440,"height":900},"navigationContentWidth":528})).unwrap();
    desktop.section=None;
    let plan=draft_plan(&[desktop.clone()], &BTreeMap::new(), &evidence(),json!({}),None).unwrap();
    assert!(array(&plan["questions"]).iter().any(|row| row["id"] == "mobile-menu"));
    let menu=screen("menu","Mobile menu",390.0);
    let plan=draft_plan(&[desktop,menu], &BTreeMap::new(), &evidence(),json!({}),None).unwrap();
    assert!(!array(&plan["questions"]).iter().any(|row| row["id"] == "mobile-menu"));
}

#[tokio::test]
async fn confirming_the_menu_recommendation_keeps_its_implementation_decision() {
    let fixture=Fixture::new();
    let mut desktop=screen("12:1","Main desktop",1440.0);
    desktop.navigation_content_width=Some(528.0);
    let mut request=drafted(&fixture,&[desktop]).await;
    request.accept_recommended=true;
    let plan=confirm(&request,&fixture.policy()).unwrap();
    assert_eq!(plan["mobileNavigation"]["mobileMenu"],"drawer");
    assert_eq!(status_report(&plan,&evidence(),None)["mobileNavigation"]["breakpoint"],"navigation-content-fit");
}

#[tokio::test]
async fn near_identical_widths_in_one_section_can_be_states_of_one_url() {
    let fixture = Fixture::new();
    let screens = [
        sectioned("30:1", "News", 1440.0, "Board"),
        sectioned("30:2", "News", 1441.0, "Board"),
        sectioned("30:3", "News", 1442.0, "Board"),
    ];
    let mut confirm_input = drafted(&fixture, &screens).await;
    let plan: Value =
        serde_json::from_slice(&std::fs::read(fixture.0.join(".devup/screen-plan.json")).unwrap())
            .unwrap();
    let asked = find(&plan, "structure:duplicate:30:1");
    assert_eq!(first_label(asked), format!("{STATES}{RECOMMENDED}"));
    confirm_input.accept_recommended = true;
    let confirmed = confirm(&confirm_input, &fixture.policy()).unwrap();
    let routes = array(&confirmed["routes"]);
    assert_eq!(routes.len(), 1, "{confirmed}");
    assert_eq!(array(&routes[0]["states"]).len(), 2);
    assert_eq!(routes[0]["states"][0]["nodeId"], "30:2");
}
#[tokio::test]
async fn r19_device_only_frames_in_each_section_confirm_as_one_responsive_url() {
    let fixture = Fixture::new();
    let screens = [
        sectioned("45:2066", "Desktop", 1440.0, "Main"),
        sectioned("51:6599", "Mobile", 360.0, "Main"),
        sectioned("60:1", "Desktop", 1440.0, "Company"),
        sectioned("60:2", "Mobile", 360.0, "Company"),
    ];
    let mut request = drafted(&fixture, &screens).await;
    request.accept_recommended = true;
    let result = confirm(&request, &fixture.policy()).unwrap();
    let routes = array(&result["routes"]);
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().all(|route| array(&route["nodeIds"]).len() == 2));
    assert!(!routes.iter().any(|route| route["url"] == "/mobile"));
}
