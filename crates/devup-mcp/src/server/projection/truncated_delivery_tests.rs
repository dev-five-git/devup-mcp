mod truncated_delivery_tests {
    use super::*;

    fn oversized_request() -> (CollectedPayload, PendingOperation) {
        let mut data = payload();
        data.metadata = json!({"large": "x".repeat(100_000)});
        let mut request = operation(&["rawPayload"]);
        if let PendingOperation::Export {
            response_selection, ..
        } = &mut request
        {
            response_selection.response_fields = vec!["rawPayload".into()];
        }
        (data, request)
    }

    fn assert_recovery_required(response: &mut Value) {
        assert_eq!(response["status"], "partial");
        assert_eq!(response["deliveryState"], "recovery-required");
        assert_eq!(response["deliverable"]["isFinal"], false);
        assert_eq!(response["deliverable"]["recoveryRequired"], true);
        assert_eq!(
            response["nextAction"],
            response["responseTruncated"]["nextAction"]
        );
        assert_eq!(
            response["nextAction"]["arguments"]["artifactId"],
            response["artifactId"]
        );
        super::super::super::review::attach(response);
        assert_eq!(response["summary"]["required"], 1);
        assert!(
            response["summary"]["next"]
                .as_str()
                .unwrap()
                .contains("required")
        );
        assert_eq!(
            response["reviewChecklist"][0]["code"],
            "DEVUP_EXPORT_RESPONSE_RECOVERY_REQUIRED"
        );
    }

    #[tokio::test]
    async fn oversized_selection_without_writes_is_partial_and_requires_recovery() {
        let (data, request) = oversized_request();
        let mut response = project(data, request).await.unwrap();
        assert_eq!(response["written"], false);
        assert_eq!(response["outputPaths"], json!({}));
        assert_recovery_required(&mut response);
    }

    #[tokio::test]
    async fn oversized_selection_keeps_committed_files_but_never_claims_final_delivery() {
        let directory = std::env::temp_dir().join(format!(
            "devup-truncated-delivery-{:016x}",
            rand::random::<u64>()
        ));
        std::fs::create_dir(&directory).unwrap();
        let target = directory.join("payload.json");
        let (data, mut request) = oversized_request();
        if let PendingOperation::Export { output_paths, .. } = &mut request {
            output_paths.insert("rawPayload".into(), target.to_string_lossy().into_owned());
        }
        let mut response = project(data, request).await.unwrap();
        assert_eq!(response["written"], true);
        assert_eq!(
            response["outputPaths"]["rawPayload"],
            json!(crate::test_paths::canonical(&target))
        );
        let saved: Value = serde_json::from_slice(&std::fs::read(&target).unwrap()).unwrap();
        assert_eq!(saved["metadata"]["large"].as_str().unwrap().len(), 100_000);
        assert_recovery_required(&mut response);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
