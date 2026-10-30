use super::*;

/// referencePng for several bridge frames is refused with one runnable
/// referencePng-only call per frame, linked through the bridge, and the other
/// outputs kept on the multi-frame call instead of being split per frame.
#[tokio::test]
async fn a_multi_frame_reference_png_refusal_hands_back_runnable_bridge_calls() -> anyhow::Result<()>
{
    let setup = setup();
    let _jobs = serve(attach(&setup.bridge, keyless_hello("Landing")).await);
    let (failed, value) = call(
        Auth::new(AuthStatus::Disconnected),
        setup.upstream.clone(),
        "devup_figma_export",
        json!({"frameIds": ["1:2", "1:3"], "outputs": ["responsiveTsx", "referencePng"]}),
    )
    .await?;
    assert!(failed, "{value}");
    let details = &value["error"]["details"];
    assert_eq!(details["recoveryState"], "available", "{value}");
    let next = &details["nextAction"]["arguments"];
    assert_eq!(next["url"], "figma-bridge://current?node-id=1-2");
    assert_eq!(next["outputs"], json!(["referencePng"]));
    assert!(details["nextAction"].get("requiredArguments").is_none());
    let urls = details["referenceCalls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|call| call["arguments"]["url"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        urls,
        vec![
            json!("figma-bridge://current?node-id=1-2"),
            json!("figma-bridge://current?node-id=1-3")
        ]
    );
    let remaining = &details["remainingCall"]["arguments"];
    assert_eq!(remaining["outputs"], json!(["responsiveTsx"]));
    assert_eq!(remaining["frameIds"], json!(["1:2", "1:3"]));
    assert_eq!(setup.remote_calls.load(Ordering::SeqCst), 0);
    Ok(())
}
