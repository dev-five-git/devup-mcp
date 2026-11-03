use super::*;
struct Project {
    root: PathBuf,
    policy: Option<OutputPolicy>,
}
impl Project {
    fn new(current: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "devup-source-sync-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(root.join(".devup/exports")).unwrap();
        std::fs::write(root.join("Screen.tsx"), current).unwrap();
        std::fs::write(root.join(".devup/exports/frame.json"),json!({"kind":"devup-export-record","generatedSource":"export function Screen(){return <Box gap=\"8px\"><Text color=\"$primary\">Count</Text></Box>}","nodes":{"frame":{"elementIndex":0},"count":{"elementIndex":1}}}).to_string()).unwrap();
        let policy = Some(OutputPolicy::from_roots(vec![root.clone()]).unwrap());
        Self { root, policy }
    }
    fn input(&self) -> SourceSyncInput {
        SourceSyncInput{action:"plan".into(),project_root:self.root.to_string_lossy().into(),baseline_record:".devup/exports/frame.json".into(),incoming_tsx:"export function Screen(){return <Box gap=\"16px\"><Text color=\"$secondary\">Count</Text></Box>}".into(),files:vec!["Screen.tsx".into()],expected_hashes:BTreeMap::new(),expected_plan_hash:None}
    }
    fn run(&self, input: SourceSyncInput) -> Value {
        run(input, self.policy.as_ref().unwrap()).unwrap()
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        drop(self.policy.take());
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn dry_run_then_atomic_apply_keeps_handlers_api_and_aria() {
    let current = "import {getCount} from './api'; export async function Screen(){const count=await getCount();return <Box gap=\"8px\" aria-label=\"Statistics\"><Text color=\"$primary\" onClick={track} tabIndex={0}>Count</Text></Box>}";
    let project = Project::new(current);
    let plan = project.run(project.input());
    assert_eq!(plan["applicable"], true, "{plan}");
    assert_eq!(
        std::fs::read_to_string(project.root.join("Screen.tsx")).unwrap(),
        current
    );
    let mut input = project.input();
    input.action = "apply".into();
    input.expected_hashes = serde_json::from_value(plan["expectedHashes"].clone()).unwrap();
    input.expected_plan_hash = plan["planHash"].as_str().map(str::to_owned);
    let result = project.run(input);
    assert_eq!(result["applied"], true);
    let source = std::fs::read_to_string(project.root.join("Screen.tsx")).unwrap();
    for preserved in [
        "await getCount()",
        "onClick={track}",
        "tabIndex={0}",
        "aria-label=\"Statistics\"",
        "import {getCount}",
    ] {
        assert!(source.contains(preserved), "{source}");
    }
    assert!(source.contains("gap=\"16px\""));
    assert!(source.contains("color=\"$secondary\""));
    assert!(project.root.join(".devup/source-mappings.json").is_file());
    let record: Value = serde_json::from_slice(
        &std::fs::read(project.root.join(".devup/exports/frame.json")).unwrap(),
    )
    .unwrap();
    assert!(
        record["generatedSource"]
            .as_str()
            .unwrap()
            .contains("gap=\"8px\"")
    );
    assert!(
        record["sourceSyncState"]["designBaseline"]
            .as_str()
            .unwrap()
            .contains("gap=\"16px\"")
    );
}
#[test]
fn conflicting_agent_style_and_stale_hash_never_overwrite() {
    let project = Project::new("<Box gap=\"12px\"><Text color=\"$primary\">Count</Text></Box>");
    let mut input = project.input();
    input.action = "apply".into();
    let result = project.run(input);
    assert_eq!(result["applied"], false);
    assert!(!result["conflicts"].as_array().unwrap().is_empty());
    assert!(
        std::fs::read_to_string(project.root.join("Screen.tsx"))
            .unwrap()
            .contains("12px")
    );
    let project = Project::new("<Box gap=\"8px\"><Text color=\"$primary\">Count</Text></Box>");
    let mut input = project.input();
    input.action = "apply".into();
    assert!(run(input, project.policy.as_ref().unwrap()).is_err());
}
#[test]
fn ambiguous_copies_and_added_topology_are_review_only() {
    let project = Project::new(
        "<Box gap=\"8px\"><Text color=\"$primary\">Count</Text><Text>Count</Text></Box>",
    );
    assert_eq!(project.run(project.input())["applicable"], false);
    let mut input = project.input();
    input
        .incoming_tsx
        .push_str(";const extra=<Text>Extra</Text>");
    assert_eq!(project.run(input)["applicable"], false);
}
#[test]
fn unchanged_static_copy_merges_but_agent_copy_and_api_expressions_do_not() {
    let project = Project::new(
        "<Box gap=\"8px\"><Text color=\"$primary\" onClick={track}>Count</Text></Box>",
    );
    let mut input = project.input();
    input.incoming_tsx = input.incoming_tsx.replace(">Count<", ">Patent count<");
    let plan = project.run(input);
    assert_eq!(plan["applicable"], true, "{plan}");
    assert!(
        plan["proposedSources"]["Screen.tsx"]
            .as_str()
            .unwrap()
            .contains("Patent count")
    );
    assert!(
        plan["proposedSources"]["Screen.tsx"]
            .as_str()
            .unwrap()
            .contains("onClick={track}")
    );
    for current in [
        "<Box gap=\"8px\"><Text color=\"$primary\">Agent copy</Text></Box>",
        "<Box gap=\"8px\"><Text color=\"$primary\">{data.total}</Text></Box>",
    ] {
        let project = Project::new(current);
        let mut input = project.input();
        input.incoming_tsx = input.incoming_tsx.replace(">Count<", ">Patent count<");
        assert_eq!(project.run(input)["applicable"], false);
    }
}
#[test]
fn formatter_quotes_and_extracted_components_preserve_source_mapping() {
    let project = Project::new("export function Screen(){return <Box gap = '8px'><Count /></Box>}");
    std::fs::write(
        project.root.join("Count.tsx"),
        "export function Count(){return <Text color = '$primary' onClick={track}>Count</Text>}",
    )
    .unwrap();
    let mut input = project.input();
    input.files.push("Count.tsx".into());
    input.incoming_tsx = input.incoming_tsx.replace("gap=\"16px\"", "gap=\"8px\"");
    let plan = project.run(input);
    assert_eq!(plan["applicable"], true, "{plan}");
    assert!(
        plan["proposedSources"]["Count.tsx"]
            .as_str()
            .unwrap()
            .contains("color=\"$secondary\"")
    );
    assert!(plan["mappings"].as_array().unwrap().iter().any(|mapping| {
        mapping["file"] == "Count.tsx"
            && mapping["reasons"]
                .as_array()
                .is_some_and(|reasons| reasons.contains(&json!("equal-static-copy")))
    }));
    let mut apply = project.input();
    apply.files.push("Count.tsx".into());
    apply.incoming_tsx = apply.incoming_tsx.replace("gap=\"16px\"", "gap=\"8px\"");
    apply.action = "apply".into();
    apply.expected_hashes = serde_json::from_value(plan["expectedHashes"].clone()).unwrap();
    apply.expected_plan_hash = plan["planHash"].as_str().map(str::to_owned);
    assert_eq!(project.run(apply)["applied"], true);
    let sidecar: Value = serde_json::from_slice(
        &std::fs::read(project.root.join(".devup/source-mappings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        sidecar["files"]["Count.tsx"],
        digest(&std::fs::read(project.root.join("Count.tsx")).unwrap())
    );
    assert!(
        sidecar["mappings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|mapping| mapping["file"] == "Count.tsx"
                && mapping["elementIndex"] == 0
                && mapping["semanticEvidence"]["text"] == "Count"
                && mapping["runtimeVerified"] == false)
    );
}
#[test]
fn atomic_apply_failure_does_not_partially_change_source() {
    let current = "<Box gap=\"8px\"><Text color=\"$primary\">Count</Text></Box>";
    let project = Project::new(current);
    let plan = project.run(project.input());
    std::fs::create_dir(project.root.join(".devup/source-mappings.json")).unwrap();
    let mut input = project.input();
    input.action = "apply".into();
    input.expected_hashes = serde_json::from_value(plan["expectedHashes"].clone()).unwrap();
    input.expected_plan_hash = plan["planHash"].as_str().map(str::to_owned);
    assert!(run(input, project.policy.as_ref().unwrap()).is_err());
    assert_eq!(
        std::fs::read_to_string(project.root.join("Screen.tsx")).unwrap(),
        current
    );
}

#[test]
fn several_new_attributes_preserve_incoming_shorthand_precedence() {
    let project = Project::new("<Box gap=\"8px\"><Text color=\"$primary\">Count</Text></Box>");
    let mut input = project.input();
    input.incoming_tsx = input
        .incoming_tsx
        .replace("gap=\"16px\"", "gap=\"16px\" p=\"12px\" px=\"24px\"");
    let result = project.run(input);
    assert_eq!(result["applicable"], true, "{result}");
    let proposed = result["proposedSources"]["Screen.tsx"].as_str().unwrap();
    assert!(
        proposed.find("p=\"12px\"").unwrap() < proposed.find("px=\"24px\"").unwrap(),
        "{proposed}"
    );
    assert!(proposed.contains("gap=\"16px\""));
}
