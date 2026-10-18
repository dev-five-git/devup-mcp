impl CollectorSession {

    fn accept_variable_catalog(&mut self, result: UpstreamResult) -> Result<(), DevupError> {
        let catalog = catalog_from_result(&result)?;
        let node_id = self.root_node_id.clone().ok_or_else(|| {
            invalid_call("No root node ID available for the Figma variable batch.")
        })?;
        for variable_ids in catalog
            .variable_ids
            .chunks(self.request.batch_budget.variable_items.max(1))
        {
            self.enqueue(
                ReadToolCall::resource_batch(
                    &self.request.target.file_key,
                    &node_id,
                    ResourceBatch {
                        variable_ids: variable_ids.to_vec(),
                        styles: Vec::new(),
                    },
                ),
                Some(node_id.clone()),
                CallKind::VariableBatch,
            );
        }
        for styles in catalog
            .styles
            .chunks(self.request.batch_budget.style_items.max(1))
        {
            self.enqueue(
                ReadToolCall::resource_batch(
                    &self.request.target.file_key,
                    &node_id,
                    ResourceBatch {
                        variable_ids: Vec::new(),
                        styles: styles.to_vec(),
                    },
                ),
                Some(node_id.clone()),
                CallKind::VariableBatch,
            );
        }
        self.variable_catalog = Some(catalog);
        Ok(())
    }
}