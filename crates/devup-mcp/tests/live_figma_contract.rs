use std::io::{self, BufRead};

use devup_mcp_devup_ui::theme::variable_snapshot_from_result;
use devup_mcp_figma::{
    CollectedPayload, CollectionRequest, CollectionScope, CollectorSession, CollectorStep,
    FigmaTarget, PayloadStructure, ResourceScope, UpstreamResult, validate_payload_context,
};
use serde_json::Value;

const LIVE_URL: &str =
    "https://www.figma.com/design/85CgSws3o5XsLv7aAwWJyS/Fixture?node-id=3879-35481";

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
    request.resource_scope = ResourceScope::File;
    let mut collector = CollectorSession::new(request);

    let parts = loop {
        match collector.advance().expect("advance collector") {
            CollectorStep::Call(call) => {
                let raw = read_result(&mut input, call.call.tool_name());
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
    assert!(!payload.snapshot.nodes.is_empty());
    let variable_result = payload.variables.as_ref().expect("variable result");
    let variables = variable_snapshot_from_result(variable_result)
        .expect("official variable/style payload must parse");
    assert!(variables.local_complete);

    let structure = PayloadStructure::from_payload(&payload);
    println!(
        "{}",
        serde_json::to_string(&structure).expect("serialize structural report")
    );
}

fn read_result(lines: &mut impl Iterator<Item = io::Result<String>>, label: &str) -> Value {
    let line = lines
        .next()
        .unwrap_or_else(|| panic!("missing {label} stdin line"))
        .unwrap_or_else(|_| panic!("failed to read {label} stdin line"));
    serde_json::from_str(&line).unwrap_or_else(|_| panic!("invalid {label} JSON envelope"))
}
