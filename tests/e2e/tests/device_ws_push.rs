//! Device WebSocket receives invite_ringing push without poll.

use chrono::Utc;
use futures_util::StreamExt;
use serde_json::Value;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio_tungstenite::connect_async;

fn spawn_api(port: u16, db: &str) -> Child {
    let bin = format!(
        "{}/../../target/debug/device-api",
        env!("CARGO_MANIFEST_DIR")
    );
    Command::new(bin)
        .args([
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--database-url",
            db,
            "--ephemeral",
        ])
        .env("CANIS_DEVICE_SECRET", "ws-secret")
        .env("CANIS_STEWARD_SECRET", "ws-steward")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn api")
}

#[tokio::test]
async fn invite_pushed_on_device_ws() {
    let port = 19292u16;
    let mut child = spawn_api(port, "sqlite::memory:");
    let api = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    for _ in 0..40 {
        if client
            .get(format!("{api}/healthz"))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let ea: Value = client
        .post(format!("{api}/v1/dev/enroll"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let eb: Value = client
        .post(format!("{api}/v1/dev/enroll"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    client
        .post(format!("{api}/v1/dev/bonds"))
        .json(&serde_json::json!({
            "dog_a": ea["dog_id"],
            "dog_b": eb["dog_id"],
            "weight": 0.9
        }))
        .send()
        .await
        .unwrap();

    // connect B ws first
    let dog_b = eb["dog_id"].as_str().unwrap();
    let term_b = eb["terminal_id"].as_str().unwrap();
    let token_b = eb["token"].as_str().unwrap();
    let ws_url = format!(
        "ws://127.0.0.1:{port}/v1/ws?dog_id={dog_b}&terminal_id={term_b}&token={token_b}"
    );
    let (ws, _) = connect_async(&ws_url).await.expect("ws connect");
    let (mut _sink, mut stream) = ws.split();

    // present both
    for (e, seq) in [(&ea, 1), (&eb, 1)] {
        let dog = e["dog_id"].as_str().unwrap();
        let term = e["terminal_id"].as_str().unwrap();
        let token = e["token"].as_str().unwrap();
        client
            .post(format!("{api}/v1/presence"))
            .header("Authorization", format!("Device {term}:{token}"))
            .json(&serde_json::json!({
                "dog_id": dog,
                "terminal_id": term,
                "present": true,
                "confidence": 0.9,
                "force_band": "medium",
                "force_n": 100.0,
                "tof_mm": 400,
                "ts": Utc::now(),
                "seq": seq
            }))
            .send()
            .await
            .unwrap();
    }

    // A calls
    let dog_a = ea["dog_id"].as_str().unwrap();
    let term_a = ea["terminal_id"].as_str().unwrap();
    let token_a = ea["token"].as_str().unwrap();
    client
        .post(format!("{api}/v1/invites"))
        .header("Authorization", format!("Device {term_a}:{token_a}"))
        .json(&serde_json::json!({
            "mode": "portal",
            "to_dog": null,
            "dog_id": dog_a,
            "terminal_id": term_a
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let mut got = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            msg = stream.next() => {
                let Some(Ok(m)) = msg else { break };
                let txt = m.to_text().unwrap_or("");
                if txt.contains("invite_ringing") || txt.contains("InviteRinging") {
                    got = true;
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(got, "expected invite_ringing push on device WS");
}
