//! Call invite: dual emulated terminals, mutual bond, A Call → B lure.

use canis_edge::{EdgeAgent, EdgeConfig, EdgeUx};
use chrono::Utc;
use db::AppData;
use device_auth::SharedSecretAuthority;
use mcu_emu::{McuEmu, McuWorld};
use protocol::{DogId, TerminalId};
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::task::JoinHandle;

use axum::{
    extract::{Query, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use session::{new_invite, route_invite};

#[derive(Clone)]
struct AppState {
    data: Arc<AppData>,
    auth: SharedSecretAuthority,
}

fn extract_token(headers: &HeaderMap) -> Result<(TerminalId, String), StatusCode> {
    let raw = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let rest = raw
        .strip_prefix("Device ")
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let (tid, token) = rest.split_once(':').ok_or(StatusCode::UNAUTHORIZED)?;
    Ok((
        TerminalId(uuid::Uuid::parse_str(tid).map_err(|_| StatusCode::UNAUTHORIZED)?),
        token.into(),
    ))
}

async fn spawn_api() -> (SocketAddr, JoinHandle<()>) {
    let state = AppState {
        data: Arc::new(AppData::new()),
        auth: SharedSecretAuthority::new("e2e-secret"),
    };
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/v1/dev/enroll",
            post(
                |State(s): State<AppState>, Json(body): Json<serde_json::Value>| async move {
                    let tid = TerminalId(
                        body.get("terminal_id")
                            .and_then(|v| v.as_str())
                            .and_then(|s| uuid::Uuid::parse_str(s).ok())
                            .unwrap_or_else(uuid::Uuid::new_v4),
                    );
                    let did = DogId(
                        body.get("dog_id")
                            .and_then(|v| v.as_str())
                            .and_then(|s| uuid::Uuid::parse_str(s).ok())
                            .unwrap_or_else(uuid::Uuid::new_v4),
                    );
                    let id = s.auth.issue(tid, did);
                    Json(serde_json::json!({
                        "terminal_id": id.terminal_id,
                        "dog_id": id.dog_id,
                        "token": id.token
                    }))
                },
            ),
        )
        .route(
            "/v1/dev/bonds",
            post(
                |State(s): State<AppState>, Json(body): Json<serde_json::Value>| async move {
                    let a = DogId(uuid::Uuid::parse_str(body["dog_a"].as_str().unwrap()).unwrap());
                    let b = DogId(uuid::Uuid::parse_str(body["dog_b"].as_str().unwrap()).unwrap());
                    let w = body.get("weight").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
                    s.data.bonds.lock().bootstrap_mutual(a, b, w);
                    StatusCode::NO_CONTENT
                },
            ),
        )
        .route(
            "/v1/presence",
            post(
                |State(s): State<AppState>,
                 headers: HeaderMap,
                 Json(report): Json<protocol::PresenceReport>| async move {
                    let (tid, token) = extract_token(&headers)?;
                    if tid != report.terminal_id {
                        return Err(StatusCode::FORBIDDEN);
                    }
                    s.auth
                        .verify_pair(report.terminal_id, report.dog_id, &token)
                        .map_err(|_| StatusCode::UNAUTHORIZED)?;
                    s.data.presence.upsert(report);
                    Ok::<_, StatusCode>(StatusCode::NO_CONTENT)
                },
            )
            .get(|State(s): State<AppState>| async move {
                Json(s.data.presence.list_present(Utc::now()))
            }),
        )
        .route(
            "/v1/invites",
            post(
                |State(s): State<AppState>,
                 headers: HeaderMap,
                 Json(body): Json<serde_json::Value>| async move {
                    let (tid, token) = extract_token(&headers)?;
                    let dog_id =
                        DogId(uuid::Uuid::parse_str(body["dog_id"].as_str().unwrap()).unwrap());
                    let terminal_id = TerminalId(
                        uuid::Uuid::parse_str(body["terminal_id"].as_str().unwrap()).unwrap(),
                    );
                    if tid != terminal_id {
                        return Err(StatusCode::FORBIDDEN);
                    }
                    s.auth
                        .verify_pair(terminal_id, dog_id, &token)
                        .map_err(|_| StatusCode::UNAUTHORIZED)?;
                    let now = Utc::now();
                    let present = s.data.presence.present_dog_ids(now);
                    let caller_present = s.data.presence.is_present(dog_id, now);
                    let bonds = s.data.bonds.lock();
                    let to = route_invite(dog_id, None, &bonds, &present, caller_present)
                        .map_err(|_| StatusCode::NOT_FOUND)?;
                    drop(bonds);
                    let invite = new_invite(dog_id, to, protocol::InviteMode::Portal);
                    s.data
                        .invites
                        .insert(invite.clone())
                        .map_err(|_| StatusCode::CONFLICT)?;
                    Ok::<_, StatusCode>(Json(serde_json::json!({ "invite": invite })))
                },
            ),
        )
        .route(
            "/v1/invites/incoming",
            get(
                |State(s): State<AppState>,
                 headers: HeaderMap,
                 Query(q): Query<std::collections::HashMap<String, String>>| async move {
                    let (tid, token) = extract_token(&headers)?;
                    let dog_id = DogId(uuid::Uuid::parse_str(&q["dog_id"]).unwrap());
                    let terminal_id = TerminalId(uuid::Uuid::parse_str(&q["terminal_id"]).unwrap());
                    if tid != terminal_id {
                        return Err(StatusCode::FORBIDDEN);
                    }
                    s.auth
                        .verify_pair(terminal_id, dog_id, &token)
                        .map_err(|_| StatusCode::UNAUTHORIZED)?;
                    let offer = s.data.invites.incoming_for(dog_id).map(|invite| {
                        serde_json::json!({
                            "invite": invite,
                            "lure": {
                                "max_repeats": 3,
                                "audio_ms": 2000,
                                "led_pattern": "slow_pulse_blue"
                            }
                        })
                    });
                    Ok::<_, StatusCode>(Json(offer))
                },
            ),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, handle)
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
    fn new(e: Enroll, api: &str) -> Self {
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
    async fn drive(&mut self, n: usize) {
        for _ in 0..n {
            let bytes = self.mcu.tick_50ms();
            let (snaps, intents) = self.edge.ingest_uart(&bytes, 50);
            for s in snaps {
                if s.flipped {
                    self.edge.publish_now().await.unwrap();
                }
            }
            for i in intents {
                if matches!(i, protocol::Intent::Call) {
                    self.edge.call(None).await.unwrap();
                }
            }
            self.edge.publish_now().await.unwrap();
        }
    }
}

#[tokio::test]
async fn call_routes_to_bonded_present_peer_with_lure() {
    let (addr, _) = spawn_api().await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    let ea: Enroll = client
        .post(format!("{base}/v1/dev/enroll"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let eb: Enroll = client
        .post(format!("{base}/v1/dev/enroll"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    client
        .post(format!("{base}/v1/dev/bonds"))
        .json(&serde_json::json!({
            "dog_a": ea.dog_id,
            "dog_b": eb.dog_id,
            "weight": 0.8
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let mut a = Term::new(ea, &base);
    let mut b = Term::new(eb, &base);
    a.mcu.set_world(McuWorld::dog_on_mat(120.0));
    b.mcu.set_world(McuWorld::dog_on_mat(100.0));
    a.drive(25).await;
    b.drive(25).await;

    a.mcu.press_pad(0);
    a.drive(2).await;
    assert_eq!(a.edge.ux, EdgeUx::RingingOut);

    let mut got = false;
    for _ in 0..10 {
        if let Some(o) = b.edge.poll_incoming().await.unwrap() {
            assert_eq!(o.invite.from_dog, a.dog_id);
            assert_eq!(o.lure.led_pattern, "slow_pulse_blue");
            assert_eq!(b.edge.ux, EdgeUx::RingingIn);
            got = true;
            break;
        }
    }
    assert!(got, "peer must receive dog-native lure");
}
