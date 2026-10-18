/// Apply at the delivery boundary so diagnostics, evidence, and issues share
/// the same ID contract, including resource-delivered diagnostic containers.
fn attach_diagnostic_screens(value: &mut Value, owners: &BTreeMap<String, Option<String>>) {
    match value {
        Value::Array(items) => {
            for item in items {
                attach_diagnostic_screens(item, owners);
            }
        }
        Value::Object(object) => {
            if object.get("code").and_then(Value::as_str).is_some()
                && object.get("message").and_then(Value::as_str).is_some()
            {
                let owner = object
                    .get("nodeId")
                    .and_then(Value::as_str)
                    .and_then(|id| owners.get(id))
                    .and_then(|id| id.as_deref());
                object.insert("screenId".into(), json!(owner));
                if owner.is_none() {
                    object.insert("screenIdReason".into(), json!("This diagnostic has no node in a known selectable screen; no screen ID was inferred."));
                }
            }
            for child in object.values_mut() {
                attach_diagnostic_screens(child, owners);
            }
        }
        _ => {}
    }
}
