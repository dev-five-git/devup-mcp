#[cfg(test)]
mod selection_contract_tests {
    use super::*;

    fn plan() -> Value {
        json!({"baseUrl":"http://127.0.0.1:3000","allowedOrigins":["http://127.0.0.1:3000"],"viewports":[{"width":360,"height":800}],"screens":[{"id":"news","routePath":"/news","steps":[{"action":"back"},{"action":"forward"},{"action":"reload"},{"action":"press","target":{"role":"link","name":"List"},"value":"Enter"}],"expect":{"selected":{"group":{"role":"navigation","name":"View"},"target":{"role":"link","name":"List"},"attribute":"aria-current","value":"true","styles":{"background-color":"rgb(0, 87, 187)"}}},"checks":[{"kind":"page-health"}]}]})
    }
    #[test]
    fn accepts_history_keyboard_and_paint_selection_expectations() {
        let plan: PlanInput = serde_json::from_value(plan()).unwrap();
        assert!(validate_plan(&plan).is_ok());
    }
    #[test]
    fn rejects_unverifiable_or_invalid_selection_expectations() {
        for invalid_selection in [json!({"styles":{}}),json!({"value":"false"}),json!({"attribute":"onclick"}),json!({"styles":{"width":"10px"}})] {
            let mut value=plan();
            for (key,value2) in invalid_selection.as_object().unwrap() {value["screens"][0]["expect"]["selected"][key]=value2.clone();}
            let plan: PlanInput=serde_json::from_value(value).unwrap();
            assert!(validate_plan(&plan).is_err());
        }
        let mut value=plan();
        value["screens"][0]["steps"][0]["target"]=json!({"role":"button","name":"Other"});
        assert!(validate_plan(&serde_json::from_value(value).unwrap()).is_err());
    }
}
