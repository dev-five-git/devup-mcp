use devup_mcp_figma::ReadToolCall;
use std::process::Command;
#[test]
fn r8_capture_distinguishes_scroll_none_absent_and_failed() {
    for call in [
        ReadToolCall::fast_snapshot("test", "1:1"),
        ReadToolCall::snapshot_chunk("test", "1:1", Default::default()),
    ] {
        let args = call.arguments();
        let script = args["code"].as_str().unwrap();
        for value in ["'VERTICAL_SCROLLING'", "'NONE'", "undefined", "'throw'"] {
            let js = format!(
                r#"const root={{id:'1:1',type:'FRAME',name:'Reader',children:[]}};
        if ({value} !== undefined) Object.defineProperty(root,'overflowDirection',{{get(){{if({value}==='throw')throw Error('denied'); return {value};}}}});
        globalThis.figma={{fileKey:'test',getNodeByIdAsync:async()=>root}};
        (async()=>{{{script}}})().then(x=>console.log(JSON.stringify(x))).catch(e=>{{console.error(e);process.exit(1)}});"#
            );
            let out = Command::new("node").args(["-e", &js]).output().unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let d: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            let n = if d.get("snapshot").is_some() {
                &d["snapshot"]["nodes"][0]
            } else {
                &d["nodes"][0]
            };
            if value == "'throw'" {
                assert!(
                    n["fieldErrors"]["overflowDirection"]
                        .as_str()
                        .unwrap()
                        .contains("denied")
                );
            } else if value == "undefined" {
                assert_eq!(n["fields"]["overflowDirection"], serde_json::Value::Null);
                assert!(
                    n["fields"].get("overflowDirection").is_some(),
                    "absence must be explicit: {n}"
                );
            } else {
                assert_eq!(n["fields"]["overflowDirection"], value.trim_matches('\''));
            }
        }
    }
}
