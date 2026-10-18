impl CollectorSession {

    fn accept_metadata(
        &mut self,
        planned: &PlannedCall,
        result: UpstreamResult,
    ) -> Result<(), DevupError> {
        let metadata = metadata_from_result_for_target(
            &result,
            &self.request.target.file_key,
            planned.expected_node_id.as_deref(),
        )?;
        if let MetadataResult::TopLevelPages(pages) = metadata {
            if planned.expected_node_id.is_some() {
                return Err(invalid_call(
                    "A page metadata request returned the top-level page list.",
                ));
            }
            self.record_metadata(result.raw);
            self.root_node_id = pages.first().map(|page| page.id.clone());
            for page in &pages {
                self.record_metadata_node(page);
                if !self.metadata_root_ids.contains(&page.id) {
                    self.metadata_root_ids.push(page.id.clone());
                }
            }
            if let Some(options) = self.request.search.clone() {
                for page in pages {
                    self.enqueue(
                        ReadToolCall::search_snapshot(
                            &self.request.target.file_key,
                            &page.id,
                            options.clone(),
                        ),
                        Some(page.id),
                        CallKind::Snapshot,
                    );
                }
                return Ok(());
            }
            if self.request.variables_only {
                return Ok(());
            }
            for page in pages {
                self.enqueue(
                    ReadToolCall::metadata(&self.request.target.file_key, Some(&page.id)),
                    Some(page.id),
                    CallKind::Metadata,
                );
            }
            return Ok(());
        }
        let MetadataResult::Document(document) = metadata else {
            unreachable!("top-level page metadata is handled above")
        };
        if document.file_key != self.request.target.file_key {
            return Err(invalid_call(
                "Figma metadata file key does not match the request.",
            ));
        }
        if let (Some(existing), Some(incoming)) = (&self.source_version, &document.version)
            && existing != incoming
        {
            return Err(DevupError::new(
                ErrorCode::DevupFigmaVersionChanged,
                "The Figma file version changed during metadata collection.",
                true,
            ));
        }
        if document.version.is_some() {
            self.source_version = document.version.clone();
        }
        if self.root_node_id.is_none() {
            self.root_node_id = Some(document.root_id.clone());
        }
        if !self.metadata_root_ids.contains(&document.root_id) {
            self.metadata_root_ids.push(document.root_id.clone());
        }
        for node in &document.nodes {
            self.record_metadata_node(node);
        }
        self.record_metadata(result.raw);
        let root = document.root().ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupFigmaNodeNotFound,
                "Target node not found in the Figma metadata.",
                false,
            )
        })?;
        if let Some(options) = self.request.search.clone() {
            self.enqueue(
                ReadToolCall::search_snapshot(&self.request.target.file_key, &root.id, options),
                Some(root.id.clone()),
                CallKind::Snapshot,
            );
            return Ok(());
        }
        if self.request.metadata_only || self.request.variables_only {
            return Ok(());
        }
        let split = !root.children_ids.is_empty()
            && (root.descendant_count > LARGE_SUBTREE_THRESHOLD
                || matches!(
                    self.request.scope,
                    CollectionScope::Page | CollectionScope::File
                ));
        let mut targets = if split {
            root.children_ids.clone()
        } else {
            vec![root.id.clone()]
        };
        // 노드 스코프에서 변환 대상은 루트 자신이다. 자식으로 쪼갠 뒤 루트를 빼면
        // 투영 단계가 대상을 찾지 못한다.
        if split && self.request.scope == CollectionScope::Node && !targets.contains(&root.id) {
            targets.push(root.id.clone());
        }
        for node_id in targets {
            self.enqueue(
                ReadToolCall::snapshot(
                    &self.request.target.file_key,
                    &node_id,
                    BuiltinScript::NodeSnapshot,
                ),
                Some(node_id),
                CallKind::Snapshot,
            );
        }
        Ok(())
    }
}