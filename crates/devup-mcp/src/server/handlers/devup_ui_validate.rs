use super::*;

#[tool_router(router = devup_ui_validate_router, vis = "pub(super)")]
impl DevupServer {
    #[tool(
        output_schema = permissive_object_output_schema()
    )]
    #[doc = include_str!("../text/devup-ui-validate.md")]
    pub(super) async fn devup_ui_validate(
        &self,
        Parameters(input): Parameters<UiValidateInput>,
    ) -> Result<CallToolResult, ErrorData> {
        use devup_mcp_devup_ui::ui_validate::{bundle_theme, check_bundle, validate_bundle};
        let invalid = |message: String| ErrorData::invalid_params(message, None);
        // Neither source supplied. This used to validate the empty string and
        // answer `ok: true, okReason: "clean"` — which reads as "this code
        // passed" about code that was never sent. There is no verdict to give
        // here, so the call is refused instead of being given a passing one.
        if input.files.as_ref().is_none_or(|files| files.is_empty()) && input.tsx.trim().is_empty()
        {
            return Err(invalid(
                "Supply either tsx or files; neither was provided, so there is nothing to validate."
                    .into(),
            ));
        }
        let theme_lookup = if let Some(files) = &input.files {
            check_bundle(files).map_err(invalid)?;
            if !input.tsx.is_empty() {
                return Err(invalid("Supply either tsx or files, not both".into()));
            }
            let theme = bundle_theme(files).map_err(invalid)?;
            let guardrail = theme.is_none().then(|| json!({
                "message": "No devup.json supplied in files; token checks skipped. Bundle validation never reads the filesystem."
            }));
            project_context::ThemeLookup { theme, guardrail }
        } else {
            project_context::theme_for_validation(input.project_root.as_deref())
                .map_err(to_mcp_error)?
        };
        let mut report = if let Some(files) = &input.files {
            validate_bundle(files, theme_lookup.theme.as_ref(), input.strict).map_err(invalid)?
        } else {
            devup_mcp_devup_ui::ui_validate::validate_devup_ui_tsx(
                &input.tsx,
                theme_lookup.theme.as_ref(),
                input.strict,
            )
        };
        // Site completeness, for code checked against a project: feedback on
        // every interactive element (bundle mode checks it per file), and an
        // icon for the app.
        if let Some(root) = input.project_root.as_deref() {
            if input.files.is_none() {
                report.violations.extend(
                    devup_mcp_devup_ui::ui_validate::site::interaction_findings(
                        input.source_name.as_deref().unwrap_or("input.tsx"),
                        &input.tsx,
                    ),
                );
            }
            report
                .violations
                .extend(screen_plan::missing_app_icon(root));
            if input.strict
                && report.violations.iter().any(|finding| {
                    finding.severity == devup_mcp_devup_ui::ui_validate::Severity::Warning
                })
            {
                report.ok = false;
            }
        }
        if input.asset_policies.len() > 4096 {
            return Err(invalid("assetPolicies exceeds 4096 entries.".into()));
        }
        let sources = input
            .files
            .as_ref()
            .map(|files| {
                files
                    .iter()
                    .map(|file| (file.path.as_str(), file.content.as_str()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![("", input.tsx.as_str())]);
        for &(name, tsx) in &sources {
            for diagnostic in
                devup_mcp_devup_ui::codegen::validate_asset_effects(tsx, &input.asset_policies)
            {
                let mut context = diagnostic
                    .details
                    .and_then(|v| v.as_object().cloned())
                    .unwrap_or_default();
                if !name.is_empty() {
                    context.insert("sourceName".into(), json!(name));
                }
                report.violations.push(devup_mcp_devup_ui::ui_validate::Violation {rule:"double-effect",severity:devup_mcp_devup_ui::ui_validate::Severity::Warning,byte_range:[0,0],context,message:diagnostic.message,suggestion:Some("Remove the repeated CSS effect, or acquire original-source and apply that effect in CSS.".into())});
                if input.strict {
                    report.ok = false;
                }
            }
        }
        // A mask flattens its SVG to one colour, which only the file shows.
        let masks = mask_guard::violations(&sources, input.project_root.as_deref());
        if input.strict && !masks.is_empty() {
            report.ok = false;
        }
        report.violations.extend(masks);
        // A reference with no file behind it renders an empty box, which only
        // the disk shows.
        let missing = asset_guard::violations(&sources, input.project_root.as_deref());
        if !missing.is_empty() {
            report.ok = false;
        }
        report.violations.extend(missing);
        if let Some(coverage) = &input.section_coverage {
            if coverage.node_ids.len() + coverage.asset_urls.len() > 4096 {
                return Err(invalid("sectionCoverage exceeds 4096 entries.".into()));
            }
            let mut missing = asset_guard::coverage_violations(&sources, &coverage.node_ids);
            missing.extend(asset_guard::asset_coverage_violations(
                &sources,
                &coverage.asset_urls,
            ));
            if !missing.is_empty() {
                report.ok = false;
            }
            report.violations.extend(missing);
        }
        // These keys are assembled here rather than serialized from
        // `UiValidation`, so anything the struct's own documentation
        // explains reaches nobody unless it is answered here too. Three
        // things in this response cannot be read off it:
        //
        // `ok: true` next to ten violations looks like a contradiction
        // until you know severity decides it and count does not, so the
        // severity split is reported and `okReason` names the case.
        //
        // `checkedTokens: 4` next to `availableTokenCount: 81` reads as
        // "4 of 81 checked". It is not: one counts references the code
        // makes, the other definitions the theme holds. `tokens` says
        // which is which in the key names, where a reader cannot miss it.
        //
        // And with no theme the token check is skipped rather than passed,
        // which a bare `checkedTokens` cannot distinguish.
        use devup_mcp_devup_ui::ui_validate::Severity;
        let count_severity = |severity| {
            report
                .violations
                .iter()
                .filter(|finding| finding.severity == severity)
                .count()
        };
        let errors = count_severity(Severity::Error);
        let warnings = count_severity(Severity::Warning);
        let infos = count_severity(Severity::Info);
        // `clean` has to mean "checked, and nothing was wrong". With no theme
        // resolved the unknown-token check never ran, so code that references
        // tokens is unverified rather than clean. This is the monorepo case
        // that answered `clean` for a `$bg` reference while no devup.json had
        // been read at all.
        let token_check_skipped = theme_lookup.theme.is_none() && report.checked_tokens > 0;
        let ok_reason = match (errors, warnings, infos, input.strict) {
            (1.., _, _, _) => "error-violations",
            (_, 1.., _, true) => "strict-warnings",
            (_, 1.., 1.., _) => "warnings-and-info",
            (_, 1.., _, _) => "warnings-only",
            (_, _, 1.., _) => "info-only",
            _ if token_check_skipped => "clean-unverified-tokens",
            _ => "clean",
        };
        let mut result = json!({
            "ok": report.ok,
            "okReason": ok_reason,
            "strict": input.strict,
            "violations": report.violations,
            "violationCounts": { "error": errors, "warning": warnings, "info": infos },
            "themeNotes": report.theme_notes,
            "checkedTokens": report.checked_tokens,
            "availableTokenCount": report.available_token_count,
            "tokens": {
                "referencedByTsx": report.checked_tokens,
                "definedByTheme": report.available_token_count,
                "unknownTokenCheckRan": theme_lookup.theme.is_some(),
            },
            "themeAvailable": theme_lookup.theme.is_some(),
            "themeGuardrail": theme_lookup.guardrail,
        });
        if let Some(name) = &input.source_name {
            for finding in result["violations"].as_array_mut().unwrap() {
                finding["sourceName"] = json!(name);
            }
        }
        attach_layout_and_source_links(&mut result, &input);
        navigation::attach_validation(&mut result, &input);
        if !report.ok {
            result.as_object_mut().unwrap().extend(
                validation_guidance::guidance(&input, &report, theme_lookup.theme.as_ref())
                    .as_object()
                    .unwrap()
                    .clone(),
            );
        }
        if let Some(action) = &report.next_action {
            result["nextAction"] = action.clone();
        }
        // A violation is a located, proven gap in devup-ui knowledge, which
        // makes this the one moment where naming the skill is a measurement
        // rather than a nudge. Raised only when the skill is actually absent:
        // telling a caller who already has it to install it is the noise that
        // teaches them to skip the field.
        let lookup = skills::Lookup::new(
            input.project_root.as_deref(),
            self.output_policy.primary_root(),
            self.client_name(),
        );
        if !report.violations.is_empty()
            && lookup.installed_paths("devup-ui").is_empty()
            && let Some(skill) = skills::find_by_name("devup-ui")
        {
            result["skillGap"] = json!({
                "skill": "devup-ui",
                "why": include_str!("../text/mod-devup_ui_validate-48eac31a.md"),
                "install": skill.install_action(&lookup),
            });
        }
        Ok(tool_result(result))
    }
}
