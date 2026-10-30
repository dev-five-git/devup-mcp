pub const PLUGIN_SCRIPTS_HASH: &str =
    "a5e954ac7dea2bf7b253b558b8821439181cf56477c0a293fa2689e18131edc0";
pub const PLUGIN_PROTOCOL: u32 = 1;

/// The scripts of [`PLUGIN_SCRIPTS_HASH`] as one evaluable expression. The bridge
/// host sends it to an open plugin whose scripts are older (`scripts-update`).
pub const PLUGIN_SCRIPTS_BUNDLE: &str = include_str!("plugin_scripts_bundle.js");

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    #[test]
    fn generated_plugin_identity_matches_shipped_sources() {
        for (source, expected) in [
            (
                include_bytes!("scripts/asset_render_helpers.js").as_slice(),
                "6d81e403e7e26889da1a58701e2d9f231edc66d65a7879f46a0d3944668b8160",
            ),
            (
                include_bytes!("scripts/assets.js").as_slice(),
                "3423f268e1dff1cf2c744f5597c403799dfb26b5cb1ed46692514532520a698d",
            ),
            (
                include_bytes!("scripts/bridge_context.js").as_slice(),
                "1b152c7d2ed67d270dc1b614d824ca5706ea0811621e66c4ab8f1fb75120746e",
            ),
            (
                include_bytes!("scripts/component_identity_helpers.js").as_slice(),
                "e9e7d0eb78286bc10e482f5fe5d46f1713fa7e1aeb9a4a3d261c8fce800d0db4",
            ),
            (
                include_bytes!("scripts/explore.js").as_slice(),
                "b4f37454a0adb35ec5b859bebc7dbd3fd18c1088fb643fb115f017e27b365bef",
            ),
            (
                include_bytes!("scripts/fast_snapshot.js").as_slice(),
                "699c69a548b6550694884a71cec99e4a1a861485dbb8848348b00f8cbae0c1fb",
            ),
            (
                include_bytes!("scripts/fast_theme.js").as_slice(),
                "ea9058356a19bddfca7208bb4c072197f47cf493172ea7e54802e654822d591d",
            ),
            (
                include_bytes!("scripts/large_value.js").as_slice(),
                "b1e3f1b59fb8fd92c3283f6c76daac9bf1f3d9459b5d1592fe30d5200a4824ea",
            ),
            (
                include_bytes!("scripts/large_value_helpers.js").as_slice(),
                "6ba07c218751f975db333406a3d3aa75ab5087de4f6895cc2f3f28529b48e204",
            ),
            (
                include_bytes!("scripts/measure_nodes.js").as_slice(),
                "e898bb09c67cf86df37703b26b412f6686996211dc9ce445ba3afabdc599a810",
            ),
            (
                include_bytes!("scripts/metadata.js").as_slice(),
                "487e4a630f443c03846b49511e2e57f04bf90279c109cc2e52cc5550a85de5e7",
            ),
            (
                include_bytes!("scripts/page_catalog.js").as_slice(),
                "d7e480407c038031431a06faa9f80bf0e85054ad47efafae316a65fdfe80e4f6",
            ),
            (
                include_bytes!("scripts/reference_png.js").as_slice(),
                "a19b172303a1af8dc454f98a0ceab1aeda8a94c413ca338e26573788f492d3d4",
            ),
            (
                include_bytes!("scripts/reference_png_chunk.js").as_slice(),
                "7251d29252d1b44aebae77b6dd588955ad01c1fe299a1e206ae7c4caa4d6b17e",
            ),
            (
                include_bytes!("scripts/search.js").as_slice(),
                "36f7815273e0545c132a1d81e6e73d2098842d27c8ff9a98440988cc1b861de0",
            ),
            (
                include_bytes!("scripts/section_index.js").as_slice(),
                "5cae43008c367227033ba54a7d52d2a0aedf1b8a5f961a4a16a2652ab8a9bbd4",
            ),
            (
                include_bytes!("scripts/snapshot.js").as_slice(),
                "8e0322175d1d8a92f44ed9baac3f667d36f4e7dc0fc86176074761c4e16aa033",
            ),
            (
                include_bytes!("scripts/used_resources.js").as_slice(),
                "b52f1e5071d8bebf8a08066d158a921b60d4b106ed7a184213f5119ab1d9afdd",
            ),
            (
                include_bytes!("scripts/variable_catalog.js").as_slice(),
                "b0be54bdc1742d3dff1ccef87970115484193eafe43d1ea8da9f3218520d7475",
            ),
            (
                include_bytes!("scripts/variables.js").as_slice(),
                "5161f2f7a6804b22cd3bb295282db448bd5688cbb0981ca95bbe017046f82f10",
            ),
            (
                include_bytes!("plugin_api_manifest.json").as_slice(),
                "a171365479755c4cd31a0e3759afe52ae8645da1a2a0446a75568f0c96c78074",
            ),
            (
                include_bytes!("text_segment_manifest.json").as_slice(),
                "6e23e63b80cbf3100dc09a5a0e05af2c1d8896bfd870f4e783af257f1d4e5864",
            ),
            (
                include_bytes!("plugin_scripts_bundle.js").as_slice(),
                "57a4c256d1001820eaddcd4aec52460ccd30d07b8b6f9f5c5ad1977bc9bf050d",
            ),
        ] {
            let actual = Sha256::digest(source)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(
                actual, expected,
                "Rebuild plugin after changing script sources"
            );
        }
    }
}
