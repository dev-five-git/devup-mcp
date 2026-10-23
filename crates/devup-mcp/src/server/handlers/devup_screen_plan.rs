use super::*;

#[tool_router(router = devup_screen_plan_router, vis = "pub(super)")]
impl DevupServer {
    #[tool(
        output_schema = permissive_object_output_schema()
    )]
    #[doc = include_str!("../text/devup-screen-plan.md")]
    pub(super) async fn devup_screen_plan(
        &self,
        Parameters(input): Parameters<screen_plan::ScreenPlanInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut result = match input.action.as_str() {
            "draft" => {
                let (screens, source) = self.plan_screens(&input).await.map_err(to_mcp_error)?;
                let mut assets = input.assets.clone();
                assets.extend(
                    source["assetCandidates"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|asset| serde_json::from_value(asset.clone()).ok()),
                );
                screen_plan::draft_with(&input, &screens, &assets, source, &self.output_policy)
                    .await
            }
            "confirm" => screen_plan::confirm(&input, &self.output_policy),
            "assume" => screen_plan::assume(&input, &self.output_policy),
            "status" => screen_plan::status(&input, &self.output_policy, &self.artifacts).await,
            "auditPlan" => {
                screen_plan::audit_plan(&input, &self.output_policy, &self.artifacts).await
            }
            "defer" => screen_plan::defer(&input, &self.output_policy),
            other => Err(DevupError::new(
                ErrorCode::DevupInvalidInput,
                format!(
                    "action must be draft, confirm, status, assume, auditPlan or defer, not {other}."
                ),
                false,
            )),
        }
        .map_err(to_mcp_error)?;
        if input.action == "auditPlan"
            && self.services.upstream.bridge_path_snapshot().await
                .is_some_and(|bridge| bridge.attached_files.len() == 1)
        {
            prefer_bridge_reference_calls(&mut result);
        }
        Ok(tool_result(result))
    }
}

fn prefer_bridge_reference_calls(value: &mut Value) {
    if value["tool"] == "devup_figma_export"
        && let Some(url) = value["arguments"]["url"].as_str()
        && let Some(node_id) = url.strip_prefix("<link to node ").and_then(|id| id.strip_suffix('>'))
    {
        let node_id = node_id.to_owned();
        value["arguments"].as_object_mut().unwrap().remove("url");
        value["arguments"]["frameIds"] = json!([node_id]);
    }
    match value {
        Value::Object(object) => object.values_mut().for_each(prefer_bridge_reference_calls),
        Value::Array(array) => array.iter_mut().for_each(prefer_bridge_reference_calls),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback57_reference_recovery_prefers_explicit_bridge_frames() {
        let mut result = json!({"missingReferences":[{"call":{"tool":"devup_figma_export",
            "arguments":{"url":"<link to node 45:2066>","outputs":["referencePng"]}}}]});
        prefer_bridge_reference_calls(&mut result);
        let arguments = &result["missingReferences"][0]["call"]["arguments"];
        assert_eq!(arguments["frameIds"], json!(["45:2066"]));
        assert!(arguments.get("url").is_none());
    }
}
