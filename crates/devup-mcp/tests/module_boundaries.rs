use std::path::PathBuf;

#[test]
fn router_keeps_projection_and_validation_implementation_in_dedicated_modules() {
    let server = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/server");
    let router = std::fs::read_to_string(server.join("mod.rs")).expect("server router source");
    let projection =
        std::fs::read_to_string(server.join("projection.rs")).expect("projection module source");
    let validation =
        std::fs::read_to_string(server.join("validation.rs")).expect("validation module source");

    for implementation in [
        "fn projected_outputs_from_result",
        "fn projection_key",
        "fn validate_artifact_projection",
        "fn validate_outputs",
    ] {
        assert!(
            !router.contains(implementation),
            "{implementation} belongs outside the MCP router"
        );
    }
    assert!(projection.contains("fn projected_outputs_from_result"));
    assert!(projection.contains("fn projection_key"));
    assert!(validation.contains("fn validate_artifact_projection"));
    assert!(validation.contains("fn validate_outputs"));
}
