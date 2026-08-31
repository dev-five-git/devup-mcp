use std::collections::BTreeSet;
use std::io::{self, BufRead};

use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::{
    CollectedPayload, CollectionRequest, CollectionScope, CollectorSession, CollectorStep,
    FigmaTarget, PayloadStructure, ResourceScope, UpstreamResult, decode_fast_snapshot,
    validate_payload_context,
};
use serde_json::{Value, json};

const LIVE_URL: &str =
    "https://www.figma.com/design/85CgSws3o5XsLv7aAwWJyS/Fixture?node-id=3879-35518";

#[test]
#[ignore = "requires official Figma MCP results on stdin"]
fn official_mcp_payload_round_trips_without_value_logging() {
    assert_eq!(
        std::env::var("DEVUP_MCP_LIVE_FIGMA").as_deref(),
        Ok("1"),
        "set DEVUP_MCP_LIVE_FIGMA=1 explicitly"
    );
    let mut input = io::stdin().lock().lines();
    let target = FigmaTarget::parse(LIVE_URL).expect("static live target");
    let mut request = CollectionRequest::new(target.clone(), CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    let mut collector = CollectorSession::new(request);
    let mut call_count = 0;

    let parts = loop {
        match collector.advance().expect("advance collector") {
            CollectorStep::Call(call) => {
                call_count += 1;
                assert_eq!(call.call.tool_name(), "use_figma");
                let raw = read_result(&mut input, call.call.tool_name());
                if call_count == 1 {
                    decode_fast_snapshot(&UpstreamResult { raw: raw.clone() }, &target)
                        .unwrap_or_else(|error| {
                            panic!("fast envelope validation failed: {}", error.details)
                        });
                }
                collector
                    .accept(&call.id, UpstreamResult { raw })
                    .expect("accept official MCP result");
            }
            CollectorStep::AwaitingResults => continue,
            CollectorStep::Complete(parts) => break parts,
        }
    };
    let payload = CollectedPayload::try_from(*parts).expect("normalize live payload");
    validate_payload_context(&payload, &target).expect("live payload context");
    let round_trip: CollectedPayload =
        serde_json::from_value(serde_json::to_value(&payload).expect("serialize live payload"))
            .expect("deserialize live payload");
    assert_eq!(round_trip, payload);
    assert_eq!(call_count, 1);
    assert_eq!(payload.stats.figma_tool_calls, 1);
    assert_eq!(payload.stats.transport, "png-envelope-v1");
    assert!(!payload.stats.fallback_used);
    assert_eq!(payload.snapshot.nodes.len(), 144);
    assert_eq!(payload.stats.node_count, 144);
    assert_eq!(payload.stats.variable_count, 20);
    assert_eq!(payload.stats.style_count, 11);
    let resources = &payload.variables.as_ref().expect("used resources").raw;
    assert_eq!(resources["variables"].as_array().map(Vec::len), Some(20));
    assert_eq!(resources["styles"].as_array().map(Vec::len), Some(11));

    let output = generate_component(
        &payload.snapshot,
        target.node_id.as_deref().expect("live node id"),
        &CodegenOptions {
            inline_instances: true,
            ..CodegenOptions::default()
        }
        .with_payload_tokens(&payload),
    )
    .expect("live DevupUI codegen");
    assert!(output.tsx.contains("borderTop=\"solid 1px $border\""));
    for typography in ["h3", "body", "bodySemibold"] {
        assert!(output.tsx.contains(&format!("typography=\"{typography}\"")));
    }

    let stats = serde_json::to_value(&payload.stats).expect("safe stats");
    let keys = stats
        .as_object()
        .expect("stats object")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        keys,
        BTreeSet::from([
            "envelopeChunks",
            "fallbackUsed",
            "figmaToolCalls",
            "nodeCount",
            "rawBytes",
            "styleCount",
            "transport",
            "variableCount",
            "wireBytes",
        ])
    );

    let structure = PayloadStructure::from_payload(&payload);
    assert_eq!(structure.node_count, 144);
    assert_eq!(structure.schema_hash.len(), 64);
    println!(
        "{}",
        json!({
            "envelopeChunks": payload.stats.envelope_chunks,
            "figmaToolCalls": payload.stats.figma_tool_calls,
            "nodeCount": structure.node_count,
            "schemaHash": structure.schema_hash,
            "styleCount": payload.stats.style_count,
            "transport": payload.stats.transport,
            "variableCount": payload.stats.variable_count,
        })
    );
}

#[test]
fn corrupted_fast_envelope_restarts_at_legacy_metadata() {
    let target = FigmaTarget::parse(LIVE_URL).expect("static live target");
    let mut request = CollectionRequest::new(target, CollectionScope::Node);
    request.resource_scope = ResourceScope::Used;
    let mut collector = CollectorSession::new(request);

    let CollectorStep::Call(fast) = collector.advance().expect("fast call") else {
        panic!("expected fast call");
    };
    assert_eq!(fast.call.tool_name(), "use_figma");
    collector
        .accept(
            &fast.id,
            UpstreamResult {
                raw: json!({"content": [{"type": "text", "text": "corrupted envelope"}]}),
            },
        )
        .expect("corruption must trigger safe fallback");

    let CollectorStep::Call(metadata) = collector.advance().expect("legacy metadata") else {
        panic!("expected legacy metadata call");
    };
    assert_eq!(metadata.call.tool_name(), "get_metadata");
}

fn read_result(lines: &mut impl Iterator<Item = io::Result<String>>, label: &str) -> Value {
    let line = lines
        .next()
        .unwrap_or_else(|| panic!("missing {label} stdin line"))
        .unwrap_or_else(|_| panic!("failed to read {label} stdin line"));
    serde_json::from_str(&line).unwrap_or_else(|_| panic!("invalid {label} JSON envelope"))
}
