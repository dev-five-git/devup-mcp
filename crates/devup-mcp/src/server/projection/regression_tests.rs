#[cfg(test)]
mod tests {
    use super::{host_safe_asset_path, with_host_safe_asset_paths};

    #[test]
    fn a_layer_name_no_file_system_takes_is_renamed_wherever_the_code_points_at_it() {
        let code = include_str!(
            "../text/projection-tests_a_layer_name_no_file_system_takes_is_renamed_wherever_the_code_p-90121fb1.md"
        );
        assert_eq!(
            with_host_safe_asset_paths(code),
            concat!(
                "<Box maskImage=\"url(/icons/ic-round-arrow-left-b041d24e668d.svg)\" />\n",
                "<Image src=\"/images/Frame-269-3e2af8571e3f.png\" />\n",
                "<Box maskImage=\"url('/icons/Frame-287-cf31d759843d.svg')\" />\n",
                "<Box maskImage=\"url(/icons/grommet-icons-language-d201f62b16f5.svg)\" />\n",
            )
        );
    }

    #[test]
    fn the_written_path_is_renamed_the_same_way_the_code_is() {
        assert_eq!(
            host_safe_asset_path("/icons/grommet-icons:language.svg"),
            "/icons/grommet-icons-language-d201f62b16f5.svg"
        );
        assert_eq!(
            host_safe_asset_path("/images/Frame 269.png"),
            "/images/Frame-269-3e2af8571e3f.png"
        );
    }
}

#[cfg(test)]
mod w1_regressions {
    include!("w1_regressions.rs");
    include!("w1_regressions_r4_export_exposes_placement_contract_without_diagnostic.rs");
    include!("w1_regressions_original_source_variant_and_css_map_resources_recover_w.rs");
    include!("w1_regressions_hero_source.rs");
    include!("w1_regressions_r7_real_wquw118_layout_contracts_are_preserved.rs");
    include!("w1_regressions_w1_hidden_asset_is_warning_and_requested_failure_is_par.rs");
}
