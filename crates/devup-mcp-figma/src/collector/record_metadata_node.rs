impl CollectorSession {

    fn record_metadata_node(&mut self, node: &crate::metadata::MetadataNode) {
        let mut fields = Map::new();
        if let Some(name) = &node.name {
            fields.insert("name".to_owned(), Value::String(name.clone()));
        }
        fields.insert(
            "childrenIds".to_owned(),
            Value::Array(
                node.children_ids
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
        fields.insert(
            "descendantCount".to_owned(),
            Value::from(node.descendant_count),
        );
        self.metadata_nodes.insert(
            node.id.clone(),
            RawNode {
                id: node.id.clone(),
                node_type: node.node_type.clone(),
                fields,
                extra: Map::new(),
                field_errors: BTreeMap::new(),
            },
        );
    }
}