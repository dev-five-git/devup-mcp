impl CollectorSession {

    fn record_unresolved_diagnostics(
        &mut self,
        refs: &UsedResourceRefs,
        unresolved: &[UnresolvedResource],
    ) {
        let unresolved = unresolved
            .iter()
            .map(|resource| (resource.kind, resource.id.as_str()))
            .collect::<BTreeSet<_>>();
        for occurrence in &refs.occurrences {
            if !unresolved.contains(&(occurrence.resource_kind, occurrence.resource_id.as_str())) {
                continue;
            }
            let diagnostic = crate::Diagnostic {
                code: "DEVUP_RESOURCE_UNRESOLVED".to_owned(),
                message: format!(
                    "Could not resolve the Figma resource; substituted the raw value: field={}, resourceId={}",
                    occurrence.field, occurrence.resource_id
                ),
                node_id: Some(occurrence.node_id.clone()),
                severity: Some(crate::DiagnosticSeverity::Warning),
                property: Some(occurrence.field.clone()),
                resource_kind: Some(
                    match occurrence.resource_kind {
                        crate::ResourceKind::Variable => "variable",
                        crate::ResourceKind::Style => "style",
                    }
                    .to_owned(),
                ),
                resource_id: Some(occurrence.resource_id.clone()),
                fallback: Some("raw-value".to_owned()),
                recoverable: Some(true),
                ..crate::Diagnostic::default()
            };
            if let Some(chunk) = self
                .snapshot_chunks
                .values_mut()
                .find(|chunk| chunk.nodes.iter().any(|node| node.id == occurrence.node_id))
            {
                chunk.diagnostics.push(diagnostic);
            }
        }
    }
}