use std::collections::{BTreeSet, HashSet};

use devup_mcp_figma::{Diagnostic, FidelityImpact, RawNode, Snapshot};
use serde_json::{Value, json};

use super::{CodegenOptions, component, layout, style, text};

pub fn instance_overrides(
    snapshot: &Snapshot,
    root_id: &str,
    options: &CodegenOptions,
) -> Vec<Value> {
    let mut pending = vec![root_id];
    let mut visited = HashSet::new();
    let mut records = Vec::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(node) = snapshot.nodes.get(id) else {
            continue;
        };
        pending.extend(node.typed_view().child_ids());
        if node.node_type == "INSTANCE" {
            records.extend(for_instance(snapshot, node, options));
        }
    }
    records
}

pub(crate) fn for_instance(
    snapshot: &Snapshot,
    instance: &RawNode,
    options: &CodegenOptions,
) -> Vec<Value> {
    let view = instance.typed_view();
    let main_id = view
        .string("mainComponentId")
        .or_else(|| view.string("componentId"));
    let main = main_id.and_then(|id| snapshot.nodes.get(id));
    let set_id = view
        .string("mainComponentSetId")
        .or_else(|| main.and_then(|node| node.typed_view().string("parentId")));
    let mut records = Vec::new();
    for entry in view
        .value("overrides")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let target_id = entry.get("id").and_then(Value::as_str);
        let target = target_id.and_then(|id| snapshot.nodes.get(id));
        let master = if target_id == Some(instance.id.as_str()) {
            main
        } else {
            target_id
                .and_then(|id| id.rsplit(';').next())
                .and_then(|id| snapshot.nodes.get(id))
                .filter(|node| Some(node.id.as_str()) != target_id)
        };
        for field in entry
            .get("overriddenFields")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if target.is_some_and(|node| expressible(instance, node, field)) {
                continue;
            }
            let resolved = target.and_then(|node| node.typed_view().value(field));
            let master_value = master.and_then(|node| node.typed_view().value(field));
            records.push(json!({
                "instanceNodeId": instance.id,
                "mainComponentId": main_id,
                "mainComponentSetId": set_id,
                "mainComponentSetName": set_id.and_then(|id| snapshot.nodes.get(id)).and_then(|node| node.typed_view().name()),
                "variantProperties": view.value("variantProperties"),
                "overriddenNodeId": target_id,
                "overriddenFields": [field],
                "classification": "not-expressible",
                "resolvedValue": resolved,
                "resolution": if resolved.is_some() { "resolved" } else { "unresolved" },
                "generatedProps": target.map(|node| generated_props(snapshot, node, options)).unwrap_or_default(),
                "generatedPropsScope": "resolved-node",
                "masterValue": master_value,
                "masterResolution": if master_value.is_some() { "resolved" } else { "unresolved" },
                "differsFromMaster": resolved.zip(master_value).map(|(value, master)| value != master),
            }));
        }
    }
    if let Some(error) = instance.field_errors.get("overrides") {
        records.push(
            json!({"instanceNodeId":instance.id,"mainComponentId":main_id,
            "classification":"not-expressible","resolution":"unresolved",
            "overriddenFields":[],"readError":error,"generatedProps":[]}),
        );
    }
    records
}

fn expressible(instance: &RawNode, target: &RawNode, field: &str) -> bool {
    let expected = match field {
        "characters" => "TEXT",
        "visible" => "BOOLEAN",
        "mainComponent" => "INSTANCE_SWAP",
        "variantProperties" => {
            return instance
                .typed_view()
                .value("componentProperties")
                .and_then(Value::as_object)
                .is_some_and(|properties| {
                    instance
                        .typed_view()
                        .value("variantProperties")
                        .and_then(Value::as_object)
                        .is_some_and(|variants| {
                            !variants.is_empty()
                                && variants.iter().all(|(key, value)| {
                                    properties.get(key).is_some_and(|prop| {
                                        prop["type"] == "VARIANT"
                                            && prop.get("value") == Some(value)
                                    })
                                })
                        })
                });
        }
        _ => return false,
    };
    target
        .typed_view()
        .value("componentPropertyReferences")
        .and_then(|refs| refs.get(field))
        .and_then(Value::as_str)
        .and_then(|key| {
            instance
                .typed_view()
                .value("componentProperties")
                .and_then(|props| props.get(key))
        })
        .is_some_and(|property| property["type"] == expected)
}

fn generated_props(snapshot: &Snapshot, node: &RawNode, options: &CodegenOptions) -> Vec<String> {
    let component = if node.node_type == "TEXT" {
        "Text"
    } else {
        "Box"
    };
    let mut props = Vec::new();
    let mut tokens = BTreeSet::new();
    layout::push_layout_props(
        snapshot,
        node,
        component,
        &mut props,
        options.root_layout,
        false,
        options.width_policy.as_ref(),
        super::image::original_image(node, &options.asset_policies),
    );
    style::push_style_props(
        snapshot,
        node,
        component,
        super::image::asset_kind(snapshot, node, &options.asset_policies),
        &mut props,
        &mut tokens,
        style::StyleOptions {
            reserve_image_content: true,
            asset_policies: &options.asset_policies,
            variable_tokens: &options.variable_tokens,
            length_tokens: &options.length_tokens,
            asset_names_per_node: options.asset_names_per_node,
            width_policy: options.width_policy.as_ref(),
        },
    );
    text::push_text_props(
        &node.typed_view(),
        &options.text_style_tokens,
        &options.typography_values,
        &options.variable_tokens,
        &mut tokens,
        &mut props,
    );
    style::tokenize_lengths(
        snapshot,
        node,
        &mut props,
        &options.length_tokens,
        &mut tokens,
    );
    props
        .iter()
        .map(|(name, component::PropValue::String(value))| {
            component::render_static_attribute(name, value)
        })
        .collect()
}

pub fn diagnostic(record: Value) -> Diagnostic {
    Diagnostic {
        code: "DEVUP_CODEGEN_INSTANCE_OVERRIDE".into(),
        message: "Instance override is not represented by component properties; preserve it through a new prop/variant or report it before replacing the inline instance.".into(),
        node_id: record["instanceNodeId"].as_str().map(str::to_owned),
        property: Some("overrides".into()),
        fidelity_impact: Some(FidelityImpact::None),
        details: Some(record),
        ..Diagnostic::default()
    }
}
