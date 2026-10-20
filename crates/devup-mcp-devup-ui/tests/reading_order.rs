use devup_mcp_devup_ui::codegen::{CodegenOptions, generate_component};
use devup_mcp_figma::Snapshot;

#[test]
fn staggered_columns_emit_one_grid_in_number_order() {
    let scene: Snapshot =
        serde_json::from_str(include_str!("fixtures/staggered-sequence.json")).unwrap();
    let out = generate_component(&scene, "root", &CodegenOptions::default()).unwrap();
    assert!(out.tsx.contains("<Grid"), "{}", out.tsx);
    let positions = ["01", "02", "03", "04"].map(|label| out.tsx.find(label).unwrap());
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{}",
        out.tsx
    );
    assert!(out.tsx.contains("gridTemplateRows={["), "{}", out.tsx);
    assert!(out.tsx.contains("gridRow={[\"auto\""), "{}", out.tsx);
}
