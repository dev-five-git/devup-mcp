use devup_mcp_figma::{
    AssetManifestEntry, AssetStatus, CompletenessState, Diagnostic, FidelityImpact,
    PayloadCompletenessReport,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AcquisitionQuality {
    Complete,
    ExpectedProjection,
    Partial,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectionQuality {
    Exact,
    Approximated,
    Lossy,
    Failed,
    NotRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeQuality {
    Complete,
    Conflicted,
    Unresolved,
    NotRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssetsQuality {
    Complete,
    Partial,
    Failed,
    NotRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputQuality {
    pub acquisition: AcquisitionQuality,
    pub projection: ProjectionQuality,
    pub theme: ThemeQuality,
    pub assets: AssetsQuality,
}

impl OutputQuality {
    pub fn status(self) -> &'static str {
        if self.acquisition == AcquisitionQuality::Failed
            || self.projection == ProjectionQuality::Failed
        {
            "failed"
        } else if matches!(
            self.acquisition,
            AcquisitionQuality::Complete | AcquisitionQuality::ExpectedProjection
        ) && matches!(
            self.projection,
            ProjectionQuality::Exact | ProjectionQuality::NotRequested
        ) && matches!(
            self.theme,
            ThemeQuality::Complete | ThemeQuality::NotRequested
        ) && matches!(
            self.assets,
            AssetsQuality::Complete | AssetsQuality::NotRequested
        ) {
            "complete"
        } else {
            "partial"
        }
    }

    pub fn strict_violation(self) -> bool {
        self.acquisition != AcquisitionQuality::Complete
            || !matches!(
                self.projection,
                ProjectionQuality::Exact | ProjectionQuality::NotRequested
            )
            || !matches!(
                self.theme,
                ThemeQuality::Complete | ThemeQuality::NotRequested
            )
            || !matches!(
                self.assets,
                AssetsQuality::Complete | AssetsQuality::NotRequested
            )
    }
}

pub fn acquisition_quality(
    report: &PayloadCompletenessReport,
    expected_projection: bool,
) -> AcquisitionQuality {
    if expected_projection {
        if !report.snapshot.missing_root_ids.is_empty() {
            AcquisitionQuality::Failed
        } else if report.snapshot.field_error_count > 0
            || !report.snapshot.truncated_fields.is_empty()
        {
            AcquisitionQuality::Partial
        } else {
            AcquisitionQuality::ExpectedProjection
        }
    } else {
        match report.state {
            CompletenessState::Complete => AcquisitionQuality::Complete,
            CompletenessState::Partial => AcquisitionQuality::Partial,
            CompletenessState::Failed => AcquisitionQuality::Failed,
        }
    }
}

pub fn projection_quality(requested: bool, diagnostics: &[Diagnostic]) -> ProjectionQuality {
    if !requested {
        return ProjectionQuality::NotRequested;
    }
    match diagnostics
        .iter()
        .map(Diagnostic::fidelity_impact)
        .max()
        .unwrap_or_default()
    {
        FidelityImpact::None => ProjectionQuality::Exact,
        FidelityImpact::Approximated => ProjectionQuality::Approximated,
        FidelityImpact::Lossy => ProjectionQuality::Lossy,
        FidelityImpact::Failed => ProjectionQuality::Failed,
    }
}

pub fn theme_quality(
    requested: bool,
    conflict_count: usize,
    unresolved_count: usize,
) -> ThemeQuality {
    if !requested {
        ThemeQuality::NotRequested
    } else if unresolved_count > 0 {
        ThemeQuality::Unresolved
    } else if conflict_count > 0 {
        ThemeQuality::Conflicted
    } else {
        ThemeQuality::Complete
    }
}

pub fn assets_quality(
    requested: bool,
    requested_asset_ids: &[String],
    assets: &[AssetManifestEntry],
) -> AssetsQuality {
    if !requested {
        return AssetsQuality::NotRequested;
    }
    if requested_asset_ids.is_empty() {
        return AssetsQuality::Complete;
    }

    let requested = requested_asset_ids
        .iter()
        .filter_map(|asset_id| assets.iter().find(|asset| &asset.asset_id == asset_id))
        .collect::<Vec<_>>();
    if requested
        .iter()
        .any(|asset| asset.status == AssetStatus::Failed)
    {
        AssetsQuality::Failed
    } else if requested.len() != requested_asset_ids.len()
        || requested
            .iter()
            .any(|asset| asset.status != AssetStatus::Exported)
    {
        AssetsQuality::Partial
    } else {
        AssetsQuality::Complete
    }
}

#[cfg(test)]
mod tests {
    use devup_mcp_figma::{Diagnostic, DiagnosticSeverity, FidelityImpact};

    use super::{ProjectionQuality, projection_quality};

    fn diagnostic(
        code: &str,
        severity: DiagnosticSeverity,
        fidelity_impact: Option<FidelityImpact>,
    ) -> Diagnostic {
        Diagnostic {
            code: code.to_owned(),
            message: "redacted fixture".to_owned(),
            severity: Some(severity),
            fidelity_impact,
            ..Diagnostic::default()
        }
    }

    #[test]
    fn projection_quality_uses_structured_fidelity_impact() {
        assert_eq!(
            projection_quality(
                true,
                &[diagnostic(
                    "IGNORED_CODE",
                    DiagnosticSeverity::Warning,
                    Some(FidelityImpact::Approximated),
                )],
            ),
            ProjectionQuality::Approximated
        );
        assert_eq!(
            projection_quality(
                true,
                &[diagnostic(
                    "IGNORED_CODE",
                    DiagnosticSeverity::Warning,
                    Some(FidelityImpact::Lossy),
                )],
            ),
            ProjectionQuality::Lossy
        );
        assert_eq!(
            projection_quality(
                true,
                &[diagnostic(
                    "IGNORED_CODE",
                    DiagnosticSeverity::Error,
                    Some(FidelityImpact::Failed),
                )],
            ),
            ProjectionQuality::Failed
        );
    }

    #[test]
    fn unknown_codegen_diagnostics_fail_closed_but_collector_warnings_do_not() {
        assert_eq!(
            projection_quality(
                true,
                &[diagnostic(
                    "DEVUP_CODEGEN_FUTURE_WARNING",
                    DiagnosticSeverity::Warning,
                    None,
                )],
            ),
            ProjectionQuality::Approximated
        );
        assert_eq!(
            projection_quality(
                true,
                &[diagnostic(
                    "DEVUP_CODEGEN_FUTURE_ERROR",
                    DiagnosticSeverity::Error,
                    None,
                )],
            ),
            ProjectionQuality::Failed
        );
        assert_eq!(
            projection_quality(
                true,
                &[diagnostic(
                    "DEVUP_RESOURCE_UNRESOLVED",
                    DiagnosticSeverity::Warning,
                    None,
                )],
            ),
            ProjectionQuality::Exact
        );
    }
}
