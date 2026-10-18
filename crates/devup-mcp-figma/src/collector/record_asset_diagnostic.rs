impl CollectorSession {

    fn record_asset_diagnostic(
        &mut self,
        request: &AssetRequest,
        error_code: &str,
        error_detail: Option<&str>,
    ) {
        if let Some(chunk) = self
            .snapshot_chunks
            .values_mut()
            .find(|chunk| chunk.nodes.iter().any(|node| node.id == request.node_id))
        {
            chunk.diagnostics.push(crate::Diagnostic {
                code: error_code.to_owned(),
                message: match error_detail {
                    Some(detail) => format!(
                        "Failed to export the requested Figma asset; layout output was kept. {detail}"
                    ),
                    None => "Failed to export the requested Figma asset; layout output was kept."
                        .to_owned(),
                },
                node_id: Some(request.node_id.clone()),
                severity: Some(crate::DiagnosticSeverity::Warning),
                property: Some(request.field.clone()),
                resource_kind: Some("asset".to_owned()),
                resource_id: Some(request.asset_id.clone()),
                fallback: Some("layout-only".to_owned()),
                recoverable: Some(true),
                ..crate::Diagnostic::default()
            });
        }
    }
}