use devup_mcp_figma::{BuiltinScript, ReadToolCall};
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn async_component_identity_and_getter_failures_survive_both_snapshot_paths() {
    for script in [
        BuiltinScript::NodeSnapshot,
        BuiltinScript::FastSnapshotEnvelope,
    ] {
        let code = ReadToolCall::snapshot("fixture", "1:1", script).arguments()["code"].clone();
        let js = r#"
const {code}=JSON.parse(require('node:fs').readFileSync(0,'utf8'));
let calls=0;
const root={id:'1:1',type:'FRAME',name:'screen',children:[]};
const main={id:'10:1',type:'COMPONENT',name:'Header / desktop',parent:{id:'10:0',type:'COMPONENT_SET'}};
root.children=[
 {id:'1:2',type:'INSTANCE',children:[],parent:root,get overrides(){return [{id:'1:2',overriddenFields:['fills']}]},get mainComponent(){throw new Error('sync main getter forbidden')},getMainComponentAsync:async()=>{calls++;return main}},
 {id:'1:3',type:'INSTANCE',children:[],parent:root,get overrides(){throw new Error('override lookup failure')},getMainComponentAsync:async()=>{calls++;throw new Error('actual main-component lookup failure')}},
 {id:'1:4',type:'INSTANCE',children:[],parent:root,getMainComponentAsync:async()=>{calls++;return null}},
];
const figma={fileKey:'fixture',getNodeByIdAsync:async()=>root,variables:{}};
const AsyncFunction=Object.getPrototypeOf(async function(){}).constructor;
new AsyncFunction('figma',code)(figma).then(data=>process.stdout.write(JSON.stringify({calls,data}))).catch(e=>{console.error(e);process.exitCode=1});
"#;
        let mut child = Command::new("node")
            .args(["-e", js])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(json!({"code":code}).to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["calls"], 3);
        let data = &result["data"];
        let nodes = data.get("snapshot").unwrap_or(data)["nodes"]
            .as_array()
            .unwrap();
        let instance = nodes.iter().find(|n| n["id"] == "1:2").unwrap();
        assert_eq!(instance["fields"]["mainComponentId"], "10:1");
        assert_eq!(instance["fields"]["mainComponentName"], "Header / desktop");
        assert_eq!(instance["fields"]["mainComponentSetId"], "10:0");
        assert_eq!(
            instance["fields"]["overrides"],
            json!([{ "id": "1:2", "overriddenFields": ["fills"] }])
        );
        assert!(instance["fieldErrors"].get("mainComponent").is_none());
        let failed = nodes.iter().find(|n| n["id"] == "1:3").unwrap();
        assert_eq!(
            failed["fieldErrors"]["overrides"],
            "override lookup failure"
        );
        assert_eq!(
            failed["fieldErrors"]["mainComponentId"],
            "actual main-component lookup failure"
        );
        assert!(failed["fields"].get("mainComponentId").is_none());
        let detached = nodes.iter().find(|n| n["id"] == "1:4").unwrap();
        assert!(detached["fields"].get("mainComponentId").unwrap().is_null());
    }
}
