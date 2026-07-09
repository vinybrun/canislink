//! GET /v1/config distributes STUN/TURN ICE for portal and edge media.

use serde_json::Value;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn spawn_api(port: u16) -> Child {
    let bin = format!(
        "{}/../../target/debug/device-api",
        env!("CARGO_MANIFEST_DIR")
    );
    Command::new(bin)
        .args([
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--database-url",
            "sqlite::memory:",
            "--ephemeral",
        ])
        .env("CANIS_DEVICE_SECRET", "ice-secret")
        .env("CANIS_STEWARD_SECRET", "ice-steward")
        .env(
            "CANIS_STUN_URLS",
            "stun:stun.l.google.com:19302,stun:stun1.l.google.com:19302",
        )
        .env(
            "CANIS_TURN_URIS",
            "turn:127.0.0.1:3478?transport=udp,turn:127.0.0.1:3478?transport=tcp",
        )
        .env("CANIS_TURN_SECRET", "lab-turn-shared-secret")
        .env("CANIS_ICE_TTL_SEC", "900")
        .env("CANIS_FORCE_TURN", "true")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn api")
}

#[tokio::test]
async fn config_distributes_turn_rest_credentials() {
    let port = 19301u16;
    let mut child = spawn_api(port);
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

    let enroll: Value = client
        .post(format!("{api}/v1/dev/enroll"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let dog = enroll["dog_id"].as_str().unwrap();
    let tid = enroll["terminal_id"].as_str().unwrap();
    let token = enroll["token"].as_str().unwrap();

    let cfg: Value = client
        .get(format!("{api}/v1/config"))
        .query(&[("dog_id", dog), ("terminal_id", tid)])
        .header("Authorization", format!("Device {tid}:{token}"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();

    let ice = &cfg["ice"];
    let stun = ice["stun_urls"].as_array().expect("stun_urls");
    assert_eq!(stun.len(), 2);
    assert!(stun[0].as_str().unwrap().starts_with("stun:"));

    let turn = ice["turn_uris"].as_array().expect("turn_uris");
    assert_eq!(turn.len(), 2);
    assert!(turn[0].as_str().unwrap().contains("turn:"));

    let user = ice["turn_username"].as_str().unwrap();
    let cred = ice["turn_credential"].as_str().unwrap();
    // coturn REST: expiry:user
    assert!(user.contains(':'), "username should be expiry:user, got {user}");
    assert!(!cred.is_empty(), "credential minted");
    assert_eq!(ice["ttl_sec"].as_u64().unwrap(), 900);

    assert_eq!(cfg["features"]["force_turn"], true);

    // Second fetch should still mint valid shape (username may share second bucket)
    let cfg2: Value = client
        .get(format!("{api}/v1/config"))
        .query(&[("dog_id", dog), ("terminal_id", tid)])
        .header("Authorization", format!("Device {tid}:{token}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!cfg2["ice"]["turn_credential"].as_str().unwrap().is_empty());

    let _ = child.kill();
}

#[tokio::test]
async fn config_stun_only_without_turn_env() {
    let port = 19302u16;
    let bin = format!(
        "{}/../../target/debug/device-api",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut child = Command::new(bin)
        .args([
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--database-url",
            "sqlite::memory:",
            "--ephemeral",
        ])
        .env("CANIS_DEVICE_SECRET", "ice-secret2")
        .env("CANIS_STEWARD_SECRET", "ice-steward2")
        // explicit empty TURN
        .env("CANIS_TURN_URIS", "")
        .env("CANIS_TURN_SECRET", "")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");

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
    let enroll: Value = client
        .post(format!("{api}/v1/dev/enroll"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let dog = enroll["dog_id"].as_str().unwrap();
    let tid = enroll["terminal_id"].as_str().unwrap();
    let token = enroll["token"].as_str().unwrap();
    let cfg: Value = client
        .get(format!("{api}/v1/config"))
        .query(&[("dog_id", dog), ("terminal_id", tid)])
        .header("Authorization", format!("Device {tid}:{token}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!cfg["ice"]["stun_urls"].as_array().unwrap().is_empty());
    assert!(cfg["ice"]["turn_uris"]
        .as_array()
        .map(|a| a.is_empty())
        .unwrap_or(true));
    assert_eq!(cfg["features"]["force_turn"], false);
    let _ = child.kill();
}
