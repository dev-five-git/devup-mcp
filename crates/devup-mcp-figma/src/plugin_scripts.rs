pub const PLUGIN_SCRIPTS_HASH: &str =
    "7d82cf8c2104ac5bed3c88cc57352a95772d757da65e8e57e6322598e9bd8bf4";
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
                "68473e139c9801808a056972c1e647ee5072d670440f6b5ea27866c67ed3ca66",
            ),
            (
                include_bytes!("scripts/assets.js").as_slice(),
                "e5678e5c3df57ee803c633a13d4d827d50c7905edd545734cbdbf9a458bee878",
            ),
            (
                include_bytes!("scripts/bridge_context.js").as_slice(),
                "626afebb51fa13f0861491cff3a6a1715a13051635ff383acdfe3944a7a3d722",
            ),
            (
                include_bytes!("scripts/component_identity_helpers.js").as_slice(),
                "942af2370e69b55aaaee5fa709decea9e209057eeb25a17d03447a04bb35aff8",
            ),
            (
                include_bytes!("scripts/explore.js").as_slice(),
                "ef323ef89972be4936e3ee5f2a559b04385d9ef5cb324c9928e9d5f4ff61f773",
            ),
            (
                include_bytes!("scripts/fast_snapshot.js").as_slice(),
                "29bd5c5c18ad62b50163065467b3c90d2034db8a8fc3cee78e93b1130d7c1fce",
            ),
            (
                include_bytes!("scripts/fast_theme.js").as_slice(),
                "4773dc0af3b7bf8d5e452efab3ea424fd84676043f5203708ed568c88d6cc42c",
            ),
            (
                include_bytes!("scripts/large_value.js").as_slice(),
                "1f590feaed7244a19b291795194ec6dc1887961f4932d24c5ecd3a8f404d8282",
            ),
            (
                include_bytes!("scripts/large_value_helpers.js").as_slice(),
                "adea569bb0b23f119c56aac675ebf0c2585583781cd7f4a1aecc09cdd15bff69",
            ),
            (
                include_bytes!("scripts/measure_nodes.js").as_slice(),
                "99a79b6595b831085685cf87a4c6fc9ea6e85dfc3945f159775c7f70eac73551",
            ),
            (
                include_bytes!("scripts/metadata.js").as_slice(),
                "d51d69c92904df6f67c678ff07fde0001f9151a1bb01be2efc588a5f8c68a7c0",
            ),
            (
                include_bytes!("scripts/page_catalog.js").as_slice(),
                "9735441c1749edf73811fd5c73f76ec6ad157224f62f79d0ff7592088a80569c",
            ),
            (
                include_bytes!("scripts/reference_png.js").as_slice(),
                "3b11a9179bc5b328880a8d5076605875e5c75e4631f6e0413c2f6dae24de48d7",
            ),
            (
                include_bytes!("scripts/reference_png_chunk.js").as_slice(),
                "278ccbe14638e3028cfd75e285e94c1f7dd904aedd26d7574fecdc874d7f4f71",
            ),
            (
                include_bytes!("scripts/search.js").as_slice(),
                "03eb7f622cf4877d59106d6cde65844b6013a464c3f2c499493dde6ed7b6a7a4",
            ),
            (
                include_bytes!("scripts/section_index.js").as_slice(),
                "4e3e0749854ca2e73cd7dab635cc31c0a1f2449027b6800de954fec078d0d161",
            ),
            (
                include_bytes!("scripts/snapshot.js").as_slice(),
                "74b582ca455a1a6be69fbe708f64fea9b2377dcc39ec65dc3428a17769be6ad3",
            ),
            (
                include_bytes!("scripts/used_resources.js").as_slice(),
                "04fd9fb4c7e9cd82fe1f0ca88f2f47bc997e5a9ba2dc3fe157b978b591f95ae6",
            ),
            (
                include_bytes!("scripts/variable_catalog.js").as_slice(),
                "367bc0f317bd988d694d8000f7405a8ec216be1b38e45604d7bdc6a01ef46972",
            ),
            (
                include_bytes!("scripts/variables.js").as_slice(),
                "b78f48e63d2d60dc9f43739637bb90b6c77e7a266a2c07379d981d5d7a045afb",
            ),
            (
                include_bytes!("plugin_api_manifest.json").as_slice(),
                "431e7c9eb6a244d486b4bebbdc60ce356fd202ea5b2661f03093b43f774c9ae6",
            ),
            (
                include_bytes!("text_segment_manifest.json").as_slice(),
                "ae43d7546b12f312303e5e984cb48268aa2ea1c4ba92e529c028bc04d74dd888",
            ),
            (
                include_bytes!("plugin_scripts_bundle.js").as_slice(),
                "70078f59c3e0eb4636c542b097e1bef21fcdb825b83e6f971672f3cef83c6533",
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
