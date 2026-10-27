use devup_mcp_figma::{BridgeServer, PLUGIN_PROTOCOL, PLUGIN_SCRIPTS_BUNDLE, PLUGIN_SCRIPTS_HASH};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

type Plugin = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn say_hello(server: &BridgeServer, hello: Value) -> Plugin {
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{}/plugin", server.port()))
        .await
        .expect("bridge accepts a plugin");
    socket
        .send(Message::Text(hello.to_string().into()))
        .await
        .expect("hello is sent");
    socket
}

async fn messages_within(socket: &mut Plugin, wait: Duration) -> Vec<Value> {
    let mut seen = Vec::new();
    while let Ok(Some(Ok(Message::Text(text)))) = tokio::time::timeout(wait, socket.next()).await {
        seen.push(serde_json::from_str(&text).expect("the server sends JSON"));
    }
    seen
}

fn hello(hash: &str, live: bool) -> Value {
    json!({
        "kind": "hello", "fileKey": "FileKey123",
        "scriptsHash": hash, "protocol": PLUGIN_PROTOCOL, "liveScripts": live,
    })
}

fn updates(messages: &[Value]) -> Vec<&Value> {
    messages
        .iter()
        .filter(|message| message["kind"] == "scripts-update")
        .collect()
}

#[tokio::test]
async fn a_live_plugin_with_older_scripts_is_sent_this_builds_scripts() {
    let server = BridgeServer::start(0).expect("an ephemeral port is free");
    let mut plugin = say_hello(&server, hello("older-build", true)).await;
    let messages = messages_within(&mut plugin, Duration::from_millis(500)).await;
    let updates = updates(&messages);
    assert_eq!(updates.len(), 1, "{messages:?}");
    assert_eq!(updates[0]["scriptsHash"], PLUGIN_SCRIPTS_HASH);
    assert_eq!(updates[0]["bundle"], PLUGIN_SCRIPTS_BUNDLE);

    // The plugin loaded them and says hello again with the new hash: no repeat.
    plugin
        .send(Message::Text(
            hello(PLUGIN_SCRIPTS_HASH, true).to_string().into(),
        ))
        .await
        .unwrap();
    let after = messages_within(&mut plugin, Duration::from_millis(300)).await;
    assert!(self::updates(&after).is_empty(), "{after:?}");
    let attached = server.state().attached_files().await;
    assert_eq!(attached[0].context.plugin_build()["state"], "current");
}

#[tokio::test]
async fn current_or_non_live_plugins_are_not_sent_scripts() {
    for hello in [
        hello(PLUGIN_SCRIPTS_HASH, true),
        hello("older-build", false),
        json!({ "kind": "hello", "fileKey": "FileKey123", "scriptsHash": "older-build",
                "protocol": PLUGIN_PROTOCOL + 1, "liveScripts": true }),
    ] {
        let server = BridgeServer::start(0).expect("an ephemeral port is free");
        let mut plugin = say_hello(&server, hello.clone()).await;
        let messages = messages_within(&mut plugin, Duration::from_millis(300)).await;
        assert!(updates(&messages).is_empty(), "{hello}: {messages:?}");
    }
}
