fn bind_text_properties(
    snapshot: &Snapshot,
    set: &RawNode,
    tree: &mut Tree,
) -> BTreeMap<String, String> {
    let properties = set.typed_view()
        .value("componentPropertyDefinitions")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, definition)| definition["type"] == "TEXT")
        .map(|(name, definition)| (
            name.clone(),
            (instance_property_name(name.split('#').next().unwrap_or(name)), definition["defaultValue"].to_string()),
        ))
        .collect::<BTreeMap<_, _>>();
    let mut defaults = properties.values().cloned().collect::<BTreeMap<_, _>>();
    let mut used = definitions(set).into_iter().map(|definition| definition.name)
        .chain(defaults.keys().cloned()).collect::<BTreeSet<_>>();
    let mut fallback_index = 0;
    let mut pending = vec![tree];
    while let Some(node) = pending.pop() {
        if let Some((name, _)) = snapshot.nodes.get(&node.node_id)
            .and_then(|raw| raw.typed_view().value("componentPropertyReferences"))
            .and_then(|references| references.get("characters"))
            .and_then(Value::as_str)
            .and_then(|reference| properties.get(reference))
        {
            node.content = Some(format!("{{{name}}}"));
        } else if let Some(raw) = snapshot.nodes.get(&node.node_id)
            && raw.node_type == "TEXT"
        {
            let layer = raw.typed_view().name().unwrap_or_default();
            let base = if layer.is_ascii() && layer.chars().any(|character| character.is_ascii_alphabetic()) {
                let mut parts = layer.split(|character: char| !character.is_ascii_alphanumeric())
                    .filter(|part| !part.is_empty());
                let mut name = parts.next().unwrap_or_default().to_owned();
                if name.chars().all(|character| !character.is_ascii_lowercase()) {
                    name.make_ascii_lowercase();
                } else if let Some(first) = name.get_mut(..1) {
                    first.make_ascii_lowercase();
                }
                for part in parts {
                    let mut part = part.to_ascii_lowercase();
                    if let Some(first) = part.get_mut(..1) {
                        first.make_ascii_uppercase();
                    }
                    name.push_str(&part);
                }
                if name.starts_with(|character: char| character.is_ascii_digit()) {
                    name.insert(0, '_');
                }
                name
            } else {
                fallback_index += 1;
                format!("text{fallback_index}")
            };
            let mut name = base.clone();
            let mut suffix = 2;
            while used.contains(&name) || matches!(name.as_str(),
                "break" | "case" | "catch" | "class" | "const" | "continue" | "debugger" |
                "default" | "delete" | "do" | "else" | "enum" | "export" | "extends" |
                "false" | "finally" | "for" | "function" | "if" | "import" | "in" |
                "instanceof" | "new" | "null" | "return" | "super" | "switch" | "this" |
                "throw" | "true" | "try" | "typeof" | "var" | "void" | "while" | "with" |
                "let" | "yield" | "await" | "implements" | "interface" | "package" |
                "private" | "protected" | "public" | "static" | "eval" | "arguments") {
                name = format!("{base}{suffix}");
                suffix += 1;
            }
            used.insert(name.clone());
            let characters = raw.typed_view().string("characters").unwrap_or_default();
            let default = match node.content.as_deref() {
                Some(content) if content.starts_with("{{") && content.ends_with('}') => content[1..content.len()-1].to_owned(),
                _ => serde_json::to_string(characters).unwrap_or_default(),
            };
            defaults.insert(name.clone(), default);
            node.content = Some(format!("{{{name}}}"));
        }
        pending.extend(node.children.iter_mut().rev());
    }
    defaults
}
