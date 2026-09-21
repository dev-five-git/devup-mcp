use std::process::Command;

/// Runs the real plugin script against a mock Figma scene graph and returns
/// whatever the harness printed.
fn run_section_index(scene: &str, report: &str) -> serde_json::Value {
    let script = include_str!("../src/scripts/section_index.js");
    // `replace` rather than `format!`: the script is dense with braces and
    // escaping every one of them for a format string hides what is tested.
    let harness = r#"
__SCENE__
globalThis.figma = { fileKey: 'test', getNodeByIdAsync: async () => section };
(async () => {
  const result = await (async () => { /*__SCRIPT__*/ })();
  console.log(JSON.stringify(__REPORT__));
})().catch((error) => console.log(JSON.stringify({ threw: error.message })));
"#
    .replace("__SCENE__", scene)
    .replace("/*__SCRIPT__*/", script)
    .replace("__REPORT__", report);
    let out = Command::new("node")
        .args(["-e", &harness])
        .output()
        .expect("node is required for plugin script regression");
    assert!(out.status.success(), "node failed: {out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    serde_json::from_str(text.trim()).unwrap_or_else(|error| panic!("{error}: {text}"))
}

/// A Section holding many screens produced a menu past the plugin transport
/// ceiling, and the script refused outright — the caller got nothing, and
/// retrying reproduced it because the ceiling follows the Section's size.
/// It now drops from the end and raises the same `projectionTruncated` the
/// traversal and candidate caps already use.
#[test]
fn an_oversized_section_menu_is_truncated_rather_than_refused() {
    let scene = r#"
const section = { id: 'sec', type: 'SECTION', name: 'Long Section', visible: true,
  absoluteBoundingBox: { x: 0, y: 0, width: 40000, height: 4000 }, children: [] };
for (let i = 0; i < 80; i += 1) {
  section.children.push({ id: 'f' + i, type: 'FRAME', visible: true, children: [],
    name: 'Screen ' + String(i).padStart(3, '0') + ' ' + 'x'.repeat(220),
    absoluteBoundingBox: { x: i * 400, y: 0, width: 360, height: 740 }, parent: section });
}
"#;
    let report = r#"{ truncated: result.nodes[0].fields.projectionTruncated,
        kept: result.nodes.length - 1,
        childIds: result.nodes[0].fields.childrenIds.length,
        bytes: JSON.stringify(result).length }"#;
    let out = run_section_index(scene, report);
    assert!(out["threw"].is_null(), "still refuses: {out}");
    assert_eq!(out["truncated"], true, "{out}");
    let kept = out["kept"].as_u64().unwrap();
    assert!(
        (1..80).contains(&kept),
        "expected a shortened menu, got {out}"
    );
    // A dropped candidate must not survive as a dangling child reference.
    assert_eq!(out["childIds"].as_u64().unwrap(), kept, "{out}");
    assert!(out["bytes"].as_u64().unwrap() <= 19 * 1024, "{out}");
}

/// Truncation is the overflow path only. A Section that fits keeps every
/// candidate and does not claim to have dropped any.
#[test]
fn a_section_that_fits_is_not_marked_truncated() {
    let scene = r#"
const section = { id: 'sec', type: 'SECTION', name: 'Short Section', visible: true,
  absoluteBoundingBox: { x: 0, y: 0, width: 4000, height: 4000 }, children: [] };
for (let i = 0; i < 3; i += 1) {
  section.children.push({ id: 'f' + i, type: 'FRAME', visible: true, children: [], name: 'Screen ' + i,
    absoluteBoundingBox: { x: i * 400, y: 0, width: 360, height: 740 }, parent: section });
}
"#;
    let report = r#"{ truncated: result.nodes[0].fields.projectionTruncated, kept: result.nodes.length - 1 }"#;
    let out = run_section_index(scene, report);
    assert_eq!(out["truncated"], false, "{out}");
    assert_eq!(out["kept"], 3, "{out}");
}

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
