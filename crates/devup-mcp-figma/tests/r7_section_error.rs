use std::process::Command;
#[test]
fn r7_section_script_reports_actual_ancestor_and_correction() {
    let script = include_str!("../src/scripts/section_index.js");
    let js = format!(
        r#"const section={{id:'4279:7806',type:'SECTION'}};const frame={{id:'3997:46333',type:'FRAME',parent:section}};globalThis.figma={{fileKey:'test',getNodeByIdAsync:async()=>frame}};(async()=>{{{script}}})().catch(e=>console.log(e.message));"#
    );
    let out = Command::new("node")
        .args(["-e", &js])
        .output()
        .expect("node is required for plugin script regression");
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    let start = text.find('{').expect("structured section recovery missing");
    let detail: serde_json::Value = serde_json::from_str(text[start..].trim()).unwrap();
    assert_eq!(detail["sectionId"], "4279:7806");
    assert_eq!(detail["nodeId"], "3997:46333");
    assert_eq!(
        detail["nextAction"]["arguments"]["url"],
        "https://www.figma.com/design/test?node-id=4279-7806"
    );
}

#[test]
fn r7_section_recovery_does_not_select_a_nested_text_node() {
    let script = include_str!("../src/scripts/section_index.js");
    let js = format!(
        r#"const section={{id:'4279:7806',type:'SECTION'}};const frame={{id:'3997:46333',type:'FRAME',parent:section}};const text={{id:'text',type:'TEXT',parent:frame}};globalThis.figma={{fileKey:'test',getNodeByIdAsync:async()=>text}};(async()=>{{{script}}})().catch(e=>console.log(e.message));"#
    );
    let out = Command::new("node").args(["-e", &js]).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    let start = text.find('{').unwrap();
    let d: serde_json::Value = serde_json::from_str(text[start..].trim()).unwrap();
    assert!(d["nextAction"]["arguments"]["frameIds"].is_null());
    assert_eq!(d["sectionId"], "4279:7806");
}
