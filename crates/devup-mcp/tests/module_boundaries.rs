use std::path::{Path, PathBuf};

fn module_sections(path: &Path) -> String {
    let source = std::fs::read_to_string(path).expect("module source");
    let mut combined = source.clone();
    for included in source.split("include!(").skip(1) {
        let Some(literal) = included.trim_start().strip_prefix('"') else {
            continue;
        };
        let relative = literal.split('"').next().unwrap();
        if relative.ends_with(".rs") {
            combined.push_str(&module_sections(&path.parent().unwrap().join(relative)));
        }
    }
    combined
}

#[test]
fn router_keeps_projection_and_validation_implementation_in_dedicated_modules() {
    let server = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/server");
    let router = module_sections(&server.join("mod.rs"));
    let projection = module_sections(&server.join("projection.rs"));
    let validation = module_sections(&server.join("validation.rs"));

    for implementation in [
        "fn projected_outputs_from_result",
        "fn projection_key",
        "fn validate_artifact_projection",
        "fn validate_outputs",
        "generate_component(",
        "generate_devup_json(",
        "std::fs::",
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
