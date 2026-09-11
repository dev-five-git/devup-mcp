use devup_mcp_figma::ReadToolCall;
use std::process::Command;

#[test]
fn r9_capture_preserves_explicit_null_without_inventing_missing_bounds() {
    for call in [
        ReadToolCall::fast_snapshot("test", "1:1"),
        ReadToolCall::snapshot_chunk("test", "1:1", Default::default()),
    ] {
        let args = call.arguments();
        let script = args["code"].as_str().unwrap();
        for value in [
            "null",
            "undefined",
            "'throw'",
            "({x:0,y:0,width:24,height:24})",
        ] {
            let js = format!(
                r#"const root={{id:'1:1',type:'FRAME',name:'Asset',children:[]}};
                if ({value} !== undefined) Object.defineProperty(root,'absoluteRenderBounds',{{get(){{if({value}==='throw')throw Error('denied');return {value};}}}});
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
            match value {
                "null" => assert_eq!(
                    n["fields"].get("absoluteRenderBounds"),
                    Some(&serde_json::Value::Null)
                ),
                "undefined" => assert!(n["fields"].get("absoluteRenderBounds").is_none()),
                "'throw'" => {
                    assert!(n["fields"].get("absoluteRenderBounds").is_none());
                    assert!(
                        n["fieldErrors"]["absoluteRenderBounds"]
                            .as_str()
                            .unwrap()
                            .contains("denied")
                    );
                }
                _ => assert_eq!(n["fields"]["absoluteRenderBounds"]["width"], 24),
            }
        }
    }
}
