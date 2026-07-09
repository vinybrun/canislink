//! Ship gate tests for base control plane.

use canis_edge::{EdgeAgent, EdgeConfig, EdgeUx};
use chrono::Utc;
use mcu_emu::{McuEmu, McuWorld};
use policy::{invite_rate_ok, social_allowed, DogPolicy};
use protocol::{DogId, SessionState, TerminalId};
use serde::Deserialize;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn spawn_api(port: u16) -> Child {
    let bin = std::env::var("CARGO_BIN_EXE_device-api").unwrap_or_else(|_| {
        format!(
            "{}/device-api",
            std::env::var("CARGO_TARGET_DIR")
                .unwrap_or_else(|_| format!(
                    "{}/target/debug",
                    env!("CARGO_MANIFEST_DIR").replace("/tests/e2e", "")
                ))
                .trim_end_matches('/')
        )
    });
    // Prefer workspace target
    let candidates = [
        bin,
        format!(
            "{}/../../target/debug/device-api",
            env!("CARGO_MANIFEST_DIR")
        ),
        "target/debug/device-api".into(),
    ];
    for c in candidates {
        if std::path::Path::new(&c).exists() {
            return Command::new(c)
                .args(["--bind", &format!("127.0.0.1:{port}")])
                .env("CANIS_DEVICE_SECRET", "ship-secret")
                .env("CANIS_STEWARD_SECRET", "ship-steward")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn");
        }
    }
    // build hint
    panic!("device-api binary not found; run cargo build -p device-api first");
}

#[derive(Deserialize)]
struct Enroll {
    terminal_id: TerminalId,
    dog_id: DogId,
    token: String,
}

struct Term {
    mcu: McuEmu,
    edge: EdgeAgent,
    dog_id: DogId,
}

impl Term {
    fn new(e: Enroll, api: &str, secret: &str) -> Self {
        // re-issue won't work — enroll returns token for ship-secret if API uses that secret
        let _ = secret;
        let dog_id = e.dog_id;
        Self {
            mcu: McuEmu::new(),
            edge: EdgeAgent::new(EdgeConfig {
                api_base: api.into(),
                terminal_id: e.terminal_id,
                dog_id: e.dog_id,
                token: e.token,
                publish_ms: 2000,
            }),
            dog_id,
        }
    }
    async fn present_and_pub(&mut self) {
        self.mcu.set_world(McuWorld::dog_on_mat(130.0));
        for _ in 0..25 {
            let b = self.mcu.tick_50ms();
            let (snaps, intents) = self.edge.ingest_uart(&b, 50);
            for s in snaps {
                if s.flipped {
                    self.edge.publish_now().await.unwrap();
                }
            }
            for i in intents {
                self.edge.handle_intent(i).await.unwrap();
            }
            self.edge.publish_now().await.unwrap();
        }
        self.edge.fetch_config().await.unwrap();
    }
}

#[tokio::test]
async fn ship_happy_path_and_estop() {
    let port = 19191u16;
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

    // Enroll with default secret won't work — API uses ship-secret.
    // Our SharedSecretAuthority uses CANIS_DEVICE_SECRET=ship-secret.
    // enroll still works (issues token with that secret).

    let ea: Enroll = client
        .post(format!("{api}/v1/dev/enroll"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let eb: Enroll = client
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
        .json(&serde_json::json!({"dog_a": ea.dog_id, "dog_b": eb.dog_id, "weight": 0.9}))
        .send()
        .await
        .unwrap();

    let mut a = Term::new(ea, &api, "ship-secret");
    let mut b = Term::new(eb, &api, "ship-secret");
    a.present_and_pub().await;
    b.present_and_pub().await;

    a.mcu.press_pad(0);
    {
        let bytes = a.mcu.tick_50ms();
        let (_, intents) = a.edge.ingest_uart(&bytes, 50);
        for i in intents {
            a.edge.handle_intent(i).await.unwrap();
        }
    }
    assert_eq!(a.edge.ux, EdgeUx::RingingOut);

    for _ in 0..10 {
        b.edge.poll_incoming().await.unwrap();
        if b.edge.ux == EdgeUx::RingingIn {
            break;
        }
    }
    assert_eq!(b.edge.ux, EdgeUx::RingingIn);

    b.mcu.press_pad(0);
    {
        let bytes = b.mcu.tick_50ms();
        let (_, intents) = b.edge.ingest_uart(&bytes, 50);
        for i in intents {
            b.edge.handle_intent(i).await.unwrap();
        }
    }
    assert!(matches!(b.edge.ux, EdgeUx::InSession | EdgeUx::Negotiating));
    a.edge.sync_active().await.unwrap();
    a.edge.report_media_ready(true).await.unwrap();
    b.edge.report_media_ready(true).await.unwrap();
    a.edge.sync_active().await.unwrap();
    assert_eq!(a.edge.session.as_ref().unwrap().state, SessionState::Active);

    // Again
    a.edge.again().await.unwrap();

    // estop
    client
        .post(format!("{api}/v1/steward/estop"))
        .header("Authorization", "Steward ship-steward")
        .json(&serde_json::json!({"dog_id": b.dog_id, "enabled": true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let s: Option<serde_json::Value> = client
        .get(format!(
            "{api}/v1/sessions/active?dog_id={}&terminal_id={}",
            a.edge.cfg.dog_id, a.edge.cfg.terminal_id
        ))
        .header("Authorization", a.edge.cfg.auth_header())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(s.is_none(), "estop must clear session");

    let _ = child.kill();
    let _ = child.wait();
}

#[tokio::test]
async fn policy_invariants() {
    let mut p = DogPolicy::default_for(DogId::new());
    assert!(social_allowed(&p, Utc::now()).is_ok());
    p.social_disabled = true;
    assert!(social_allowed(&p, Utc::now()).is_err());
    assert!(invite_rate_ok(12, 12).is_err());
}
