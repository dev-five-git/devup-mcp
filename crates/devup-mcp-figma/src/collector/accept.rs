impl CollectorSession {

    pub fn accept(&mut self, call_id: &str, result: UpstreamResult) -> Result<(), DevupError> {
        let pending = self
            .pending
            .remove(call_id)
            .ok_or_else(|| invalid_call("Unknown or already-handled Figma call ID."))?;
        self.consumed.insert(call_id.to_owned());
        if let Some(served) = crate::BridgeServed::from_result(&result.raw) {
            let reads = self.stats.bridge.as_ref().map_or(0, |bridge| bridge.reads) + served.reads;
            self.stats.bridge = Some(crate::BridgeServed { reads, ..served });
        }
        if let Some(reason) = result.raw["_meta"][crate::bridge::DIRECT_FALLBACK_META_KEY].as_str()
        {
            self.stats.direct_fallbacks.push(reason.to_owned());
        }
        match pending.kind {
            CallKind::FastSnapshot => {
                self.accept_fast_snapshot(&pending.planned, pending.order, result)
            }
            CallKind::FastTheme => self.accept_fast_theme(result),
            CallKind::Metadata => self.accept_metadata(&pending.planned, result),
            CallKind::PageCatalog => self.accept_page_catalog(&pending.planned, result),
            CallKind::Snapshot => self.accept_snapshot(&pending.planned, pending.order, result),
            CallKind::VariableCatalog => self.accept_variable_catalog(result),
            CallKind::VariableBatch => {
                let batch = batch_from_result(&result)?;
                self.enqueue_style_consumer_batches(&batch)?;
                self.variable_batches.insert(pending.order, batch);
                Ok(())
            }
            CallKind::UsedResourceBatch => {
                let batch = batch_from_result(&result)?;
                self.variable_batches.insert(pending.order, batch);
                Ok(())
            }
            CallKind::Explore | CallKind::ScopedSearch => {
                self.accept_projection(&pending.planned, pending.order, result)
            }
            CallKind::SectionIndex => {
                self.accept_section_index(&pending.planned, pending.order, result)
            }
            CallKind::FastMultiRoot => {
                self.accept_fast_multi_root(&pending.planned, pending.order, result)
            }
            CallKind::LargeValue => self.accept_large_value(&pending.planned, result),
            CallKind::Asset => self.accept_asset(&pending.planned, result),
            CallKind::Screenshot => self.accept_reference_png(&pending.planned, result),
        }
    }
}