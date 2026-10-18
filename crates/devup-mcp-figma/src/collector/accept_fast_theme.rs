impl CollectorSession {

    fn accept_fast_theme(&mut self, result: UpstreamResult) -> Result<(), DevupError> {
        let payload = match decode_fast_theme(&result, &self.request.target.file_key) {
            Ok(payload) => payload,
            Err(error) => {
                self.restart_legacy(fallback_category(&error));
                return Ok(());
            }
        };
        self.fast_theme_page_count += 1;
        self.stats.raw_bytes = self.stats.raw_bytes.saturating_add(payload.stats.raw_bytes);
        self.stats.wire_bytes = self
            .stats
            .wire_bytes
            .saturating_add(payload.stats.wire_bytes);
        // A page is merged onto the pages before it: the resources are
        // concatenated in the one order the script lists them, and the scan
        // every page repeats - which ids are used, what could not be
        // resolved - is taken from the page at hand.
        let merged = match self.fast_theme_pages.take() {
            None => payload.resources.raw,
            Some(mut merged) => {
                merge_theme_page(&mut merged, payload.resources.raw)?;
                merged
            }
        };
        if let Some(page) = payload.page
            && !page.complete
        {
            if page.next_offset <= page.offset {
                return Err(invalid_call("Figma theme page cursor did not advance."));
            }
            self.fast_theme_pages = Some(merged);
            self.enqueue(
                ReadToolCall::fast_theme_page(&self.request.target.file_key, page.next_offset),
                None,
                CallKind::FastTheme,
            );
            return Ok(());
        }
        // The remote variables among those collected, which a page does not
        // list twice.
        let mut merged = merged;
        let remote = merged["variables"]
            .as_array()
            .map(|variables| {
                variables
                    .iter()
                    .filter(|variable| {
                        variable.get("remote").and_then(Value::as_bool) == Some(true)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if merged["usedRemoteVariables"]
            .as_array()
            .is_none_or(Vec::is_empty)
        {
            merged["usedRemoteVariables"] = Value::Array(remote);
        }
        self.metadata = Some(json!({
            "transport": payload.stats.transport,
            "pageCount": self.fast_theme_page_count,
            "collectionCount": merged["collections"].as_array().map_or(0, Vec::len),
            "variableCount": merged["variables"].as_array().map_or(0, Vec::len),
            "styleCount": merged["styles"].as_array().map_or(0, Vec::len)
        }));
        self.source_version = payload.source_version;
        self.stats.transport = payload.stats.transport.to_owned();
        self.stats.envelope_chunks = self.fast_theme_page_count;
        self.variables = Some(UpstreamResult { raw: merged });
        Ok(())
    }
}