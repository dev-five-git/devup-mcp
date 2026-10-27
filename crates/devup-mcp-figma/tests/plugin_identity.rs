use devup_mcp_figma::{PLUGIN_PROTOCOL, PLUGIN_SCRIPTS_HASH, PluginContext};
use serde_json::json;

#[test]
fn plugin_identity_distinguishes_matching_mismatched_and_unknown_builds() {
    for (hash, protocol, state) in [
        (Some(PLUGIN_SCRIPTS_HASH), Some(PLUGIN_PROTOCOL), "current"),
        (Some("old-build"), Some(PLUGIN_PROTOCOL), "stale"),
        (None, None, "stale-unknown"),
        (Some(PLUGIN_SCRIPTS_HASH), Some(999), "stale"),
    ] {
        let context: PluginContext =
            serde_json::from_value(json!({"scriptsHash":hash,"protocol":protocol})).unwrap();
        let build = context.plugin_build();
        assert_eq!(build["state"], state);
        assert_eq!(build["pluginStale"], state != "current");
        assert_eq!(build["expected"], PLUGIN_SCRIPTS_HASH);
        assert_eq!(build["actual"], json!(hash));
        assert_eq!(
            serde_json::to_value(context).unwrap()["scriptsHash"],
            json!(hash)
        );
    }
}
