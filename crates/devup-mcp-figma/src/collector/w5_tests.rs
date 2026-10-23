

#[cfg(test)]
mod w5_tests {
    use super::*;
    #[test]
    fn aggregate_budget_keeps_successes_and_exact_failure_options_and_stops_remaining_reads() {
        let target =
            FigmaTarget::parse("https://www.figma.com/design/FileKey123/Fixture?node-id=1-2")
                .unwrap();
        let mut collector =
            CollectorSession::new(CollectionRequest::new(target, CollectionScope::Node));
        let request = AssetRequest {
            usage_aspect_ratio: None,
            max_width: None,
            srcset: false,
            visible_scope_node_id: None,
            asset_id: "synthetic:fills:0".into(),
            node_id: "synthetic".into(),
            field: "fills/0".into(),
            image_hash: Some("source".into()),
            format: crate::AssetFormat::Png,
            scale: 1.0,
            render_mode: crate::AssetRenderMode::Node,
            quality: None,
            lossless: None,
        };
        let mut asset =
            crate::exported_asset_from_bytes(&request, b"synthetic byte-budget boundary").unwrap();
        collector.retained_asset_bytes = MAX_COLLECTED_ASSET_BYTES - asset.byte_length.unwrap();
        collector.retain_asset(asset.clone());
        assert_eq!(collector.asset_results[0].status, AssetStatus::Exported);
        asset.asset_id = "second:fills:0".into();
        asset.quality = Some(41);
        asset.lossless = Some(true);
        asset.format = Some(crate::AssetFormat::Webp);
        collector.retain_asset(asset);
        let failed = &collector.asset_results[1];
        assert_eq!(failed.status, AssetStatus::Failed);
        assert_eq!(failed.quality, Some(41));
        assert_eq!(failed.lossless, Some(true));
        assert!(failed.data_base64.is_none());
        assert!(failed.sha256.is_none());
        assert!(collector.asset_byte_budget_exhausted);
        assert_eq!(collector.retained_asset_bytes, MAX_COLLECTED_ASSET_BYTES);
    }
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    fn run_script(script: &str, setup: &str, key: &str, options: Value) -> Value {
        let script = script
            .replace("__DEVUP_NODE_ID__", "2:1")
            .replace(&format!("\"{key}\""), &options.to_string());
        let program = format!(
            "{setup}\n(async () => {{ {script} }})().then(v => process.stdout.write(JSON.stringify(v))).catch(e => {{ console.error(e); process.exit(1); }});"
        );
        let mut child = Command::new("node")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Node.js required for script tests");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(program.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    const TREE: &str = include_str!("../text/collector-w5_tests_TREE-ae8d8ae7.md");

    fn search_output(setup: &str) -> Value {
        run_script(
            include_str!("../scripts/search.js"),
            setup,
            "__DEVUP_SEARCH__",
            json!({"query":"Loading", "nodeTypes":["FRAME"], "matchKind":"exact", "limit":5}),
        )
    }

    #[test]
    fn p4_section_keeps_long_page_and_nested_automatic_screens() {
        let setup = format!(
            include_str!(
                "../text/collector-w5_tests_p4_section_keeps_long_page_and_nested_automatic_screens-261e1062.md"
            ),
            TREE = TREE
        );
        let output = run_script(
            include_str!("../scripts/section_index.js"),
            &setup,
            "unused",
            Value::Null,
        );
        let nodes = output["nodes"].as_array().unwrap();
        assert!(
            nodes.iter().any(|n| n["id"] == "5:1"),
            "nested screen was discarded"
        );
        assert!(!nodes.iter().any(|n| n["id"] == "5:2"));
        let screen = nodes.iter().find(|n| n["id"] == "5:1").unwrap();
        assert_eq!(screen["fields"]["parentId"], "3:1");
        let snapshot = merge_chunks(vec![
            snapshot_chunk_from_result(&UpstreamResult { raw: output }).unwrap(),
        ])
        .unwrap();
        let target = FigmaTarget {
            file_key: "file".into(),
            node_id: Some("2:1".into()),
            branch_key: None,
        };
        let index = build_section_index(&snapshot, &target).unwrap();
        assert_eq!(index.select(&[], true).unwrap(), ["5:1"]);
        assert_eq!(index.select(&["3:1".into()], false).unwrap(), ["3:1"]);
        for id in ["3:1", "6:1", "6:2"] {
            let candidate = index.candidates.iter().find(|c| c.node_id == id).unwrap();
            assert!(!candidate.is_screen_candidate());
            assert!(
                candidate
                    .selection_reasons
                    .contains(&"explicit-selection-only".into())
            );
        }
    }

    #[test]
    fn w5_search_accepts_section_and_frame_roots_without_scanning_siblings() {
        for setup in [
            TREE.to_owned(),
            format!("{TREE}\nfigma.getNodeByIdAsync = async () => frame;"),
        ] {
            let output = search_output(&setup);
            let nodes = output["nodes"].as_array().unwrap();
            assert!(nodes.iter().any(|n| n["id"] == "3:1"));
            assert!(!nodes.iter().any(|n| n["id"] == "3:2"));
            assert!(nodes.iter().any(|n| n["id"] == "2:1"));
            assert_eq!(output["rootIds"], json!(["1:1"]));
        }
    }

    #[test]
    fn w5_text_preview_does_not_change_candidates_or_projection_truncation() {
        let setup = format!(
            include_str!(
                "../text/collector-w5_tests_w5_text_preview_does_not_change_candidates_or_projection_trun-90cc510e.md"
            ),
            TREE = TREE
        );
        let mut baseline = None;
        for limit in [0, 160, 500] {
            let output = run_script(
                include_str!("../scripts/explore.js"),
                &setup,
                "__DEVUP_EXPLORE__",
                json!({"projectionLimit":200,"textPreviewLimit":limit}),
            );
            let nodes = output["nodes"].as_array().unwrap();
            let ids: Vec<_> = nodes
                .iter()
                .filter(|n| n["type"] == "FRAME")
                .map(|n| n["id"].clone())
                .collect();
            assert!(!ids.is_empty());
            let snapshot = merge_chunks(vec![
                snapshot_chunk_from_result(&UpstreamResult {
                    raw: output.clone(),
                })
                .unwrap(),
            ])
            .unwrap();
            let target = FigmaTarget {
                file_key: "file".into(),
                node_id: Some("2:1".into()),
                branch_key: None,
            };
            let explored =
                crate::explore_snapshot(&snapshot, &target, &crate::ExploreOptions { limit: 100 })
                    .unwrap();
            let candidate_ids: Vec<_> = explored
                .candidates
                .iter()
                .map(|candidate| candidate.node.node_id.clone())
                .collect();
            assert_eq!(candidate_ids.len(), ids.len());
            let state = (
                ids,
                candidate_ids,
                nodes[0]["fields"]["projectionTruncated"].clone(),
            );
            if let Some(expected) = &baseline {
                assert_eq!(&state, expected);
            } else {
                baseline = Some(state);
            }
            assert!(output.to_string().chars().count() <= 14_000);
        }
    }

    #[test]
    fn r12_page_inventory_stops_at_screen_frames_instead_of_walking_their_contents() {
        let setup = format!("{TREE}\nfigma.getNodeByIdAsync = async () => page;\nfor(let index=0;index<2000;index++) node('deep:'+index, 'GROUP', 'Decorative', frame);\n");
        let output = run_script(include_str!("../scripts/explore.js"), &setup,
            "__DEVUP_EXPLORE__", json!({"projectionLimit":200,"textPreviewLimit":0}));
        let nodes = output["nodes"].as_array().unwrap();
        assert!(nodes.iter().any(|node| node["id"] == "3:1"));
        assert!(!nodes.iter().any(|node| node["id"].as_str().unwrap().starts_with("deep:")));
        assert_eq!(nodes[0]["fields"]["projectionTruncated"], false);
    }

    #[test]
    fn w5_previews_are_retained_when_the_envelope_has_room() {
        let setup =
            format!("{TREE}\nnode('4:1', 'TEXT', 'Copy', frame).characters = 'Preview text';");
        let output = run_script(
            include_str!("../scripts/explore.js"),
            &setup,
            "__DEVUP_EXPLORE__",
            json!({"projectionLimit":200,"textPreviewLimit":160}),
        );
        let nodes = output["nodes"].as_array().unwrap();
        let frame = nodes.iter().find(|node| node["id"] == "3:1").unwrap();
        assert_eq!(frame["fields"]["textPreview"], "Preview text");
        assert_eq!(nodes[0]["fields"]["projectionTruncated"], false);
    }

    #[test]
    fn w5_empty_search_preserves_anchor_and_ancestry() {
        let output = search_output(&format!("{TREE}\nframe.name = 'Unrelated';"));
        let ids: BTreeSet<_> = output["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| node["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, BTreeSet::from(["1:1", "2:1"]));
    }

    fn page_output(ids: &[String]) -> UpstreamResult {
        UpstreamResult {
            raw: json!({
                "fileKey": "file",
                "version": null,
                "rootIds": ids,
                "nodes": ids.iter().map(|id| json!({
                    "id": id, "type": "PAGE",
                    "fields": {"name": "Page", "parentId": null, "childrenIds": []},
                    "extra": {}, "fieldErrors": {}
                })).collect::<Vec<_>>(),
                "diagnostics": []
            }),
        }
    }

    #[test]
    fn w5_file_search_still_schedules_all_thirteen_pages() {
        let target = FigmaTarget {
            file_key: "file".into(),
            node_id: Some("2:1".into()),
            branch_key: None,
        };
        let mut request = CollectionRequest::new(target, CollectionScope::File);
        request.search = Some(SearchReadOptions {
            query: "Loading".into(),
            match_kind: "exact".into(),
            limit: 5,
            ..Default::default()
        });
        let mut session = CollectorSession::new(request);
        let CollectorStep::Call(call) = session.advance().unwrap() else {
            panic!("expected catalog")
        };
        assert!(matches!(call.call, ReadToolCall::PageCatalog { .. }));
        let pages: Vec<_> = (1..=13).map(|i| format!("1:{i}")).collect();
        session.accept(&call.id, page_output(&pages)).unwrap();
        let mut searched = BTreeSet::new();
        loop {
            match session.advance().unwrap() {
                CollectorStep::Call(call) => {
                    let ReadToolCall::SearchSnapshot { node_id, .. } = &call.call else {
                        panic!("expected page search")
                    };
                    searched.insert(node_id.clone());
                    session
                        .accept(&call.id, page_output(std::slice::from_ref(node_id)))
                        .unwrap();
                }
                CollectorStep::Complete(parts) => {
                    assert_eq!(parts.stats.figma_tool_calls, 14);
                    assert_eq!(parts.scope, CollectionScope::File);
                    break;
                }
                CollectorStep::AwaitingResults => panic!("all calls were accepted"),
            }
        }
        assert_eq!(searched, pages.into_iter().collect());
    }

    #[test]
    fn w5_node_search_finishes_in_one_call_without_page_catalog() {
        let target = FigmaTarget {
            file_key: "file".into(),
            node_id: Some("2:1".into()),
            branch_key: None,
        };
        let mut request = CollectionRequest::new(target, CollectionScope::Node);
        request.search = Some(SearchReadOptions {
            query: "Loading".into(),
            match_kind: "exact".into(),
            limit: 5,
            ..Default::default()
        });
        let mut session = CollectorSession::new(request);
        let CollectorStep::Call(call) = session.advance().unwrap() else {
            panic!("expected search call")
        };
        assert!(
            matches!(&call.call, ReadToolCall::SearchSnapshot {node_id, ..} if node_id == "2:1")
        );
        session
            .accept(
                &call.id,
                UpstreamResult {
                    raw: search_output(TREE),
                },
            )
            .unwrap();
        let CollectorStep::Complete(parts) = session.advance().unwrap() else {
            panic!("extra Figma call")
        };
        assert_eq!(parts.stats.figma_tool_calls, 1);
        assert_eq!(parts.scope, CollectionScope::Node);
    }

    #[test]
    fn exploration_carries_measured_header_navigation_into_screen_planning() {
        let setup = format!("{TREE}\nframe.width=1440; frame.height=900; const nav=node('nav','FRAME','Header navigation',frame); nav.layoutMode='HORIZONTAL'; nav.itemSpacing=24; for(let i=0;i<3;i++){{const item=node('item'+i,'TEXT','Link',nav);item.width=160;item.height=24;}}");
        for (script, key, options) in [
            (include_str!("../scripts/explore.js"), "__DEVUP_EXPLORE__", json!({"projectionLimit":200})),
            (include_str!("../scripts/section_index.js"), "__DEVUP_ROOT_IDS__", json!([])),
        ] {
            let output = run_script(script, &setup, key, options);
            let frame = output["nodes"].as_array().unwrap().iter().find(|node|node["id"] == "3:1").unwrap();
            assert_eq!(frame["fields"]["navigationContentWidth"], 528);
            let raw: RawNode = serde_json::from_value(frame.clone()).unwrap();
            assert_eq!(crate::ExploreNode::try_from(&raw).unwrap().navigation_content_width, Some(528.0));
        }
    }
}
