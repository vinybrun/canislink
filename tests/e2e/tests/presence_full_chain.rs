//! Full-chain presence test with in-process API + dual MCU emulators.
//!
//! Chain: McuEmu → UART bytes → EdgeAgent fusion → HTTP device-api → PresenceStore TTL

use canis_edge::{EdgeAgent, EdgeConfig};
use chrono::Utc;
use db::PresenceStore;
use device_auth::SharedSecretAuthority;
use mcu_emu::{McuEmu, McuWorld};
use protocol::{DogId, PresenceView, TerminalId, PRESENCE_TTL_MS};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::task::JoinHandle;

use axum::{
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
struct AppState {
    presence: PresenceStore,
    auth: SharedSecretAuthority,
}

#[derive(Deserialize)]
struct EnrollRequest {
    terminal_id: Option<uuid::Uuid>,
    dog_id: Option<uuid::Uuid>,
}

#[derive(Serialize, Deserialize)]
struct EnrollResponse {
    terminal_id: TerminalId,
    dog_id: DogId,
    token: String,
}

async fn spawn_api() -> (SocketAddr, JoinHandle<()>) {
    let state = AppState {
        presence: PresenceStore::new(),
        auth: SharedSecretAuthority::new("e2e-secret"),
    };
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/v1/dev/enroll",
            post(
                |State(state): State<AppState>, Json(body): Json<EnrollRequest>| async move {
                    let terminal_id =
                        TerminalId(body.terminal_id.unwrap_or_else(uuid::Uuid::new_v4));
                    let dog_id = DogId(body.dog_id.unwrap_or_else(uuid::Uuid::new_v4));
                    let id = state.auth.issue(terminal_id, dog_id);
                    Json(EnrollResponse {
                        terminal_id: id.terminal_id,
                        dog_id: id.dog_id,
                        token: id.token,
                    })
                },
            ),
        )
        .route(
            "/v1/presence",
            post(
                |State(state): State<AppState>,
                 headers: HeaderMap,
                 Json(report): Json<protocol::PresenceReport>| async move {
                    let raw = headers
                        .get(AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .ok_or(StatusCode::UNAUTHORIZED)?;
                    let rest = raw
                        .strip_prefix("Device ")
                        .ok_or(StatusCode::UNAUTHORIZED)?;
                    let (tid, token) = rest.split_once(':').ok_or(StatusCode::UNAUTHORIZED)?;
                    let terminal_id = TerminalId(
                        uuid::Uuid::parse_str(tid).map_err(|_| StatusCode::UNAUTHORIZED)?,
                    );
                    if terminal_id != report.terminal_id {
                        return Err(StatusCode::FORBIDDEN);
                    }
                    state
                        .auth
                        .verify_pair(report.terminal_id, report.dog_id, token)
                        .map_err(|_| StatusCode::UNAUTHORIZED)?;
                    state.presence.upsert(report);
                    Ok::<_, StatusCode>(StatusCode::NO_CONTENT)
                },
            )
            .get(|State(state): State<AppState>| async move {
                Json(state.presence.list_present(Utc::now()))
            }),
        )
        .route(
            "/v1/presence/{dog_id}",
            get(
                |State(state): State<AppState>,
                 axum::extract::Path(dog_id): axum::extract::Path<uuid::Uuid>| async move {
                    state
                        .presence
                        .get(DogId(dog_id), Utc::now())
                        .map(Json)
                        .ok_or(StatusCode::NOT_FOUND)
                },
            ),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, handle)
}

async fn enroll(base: &str) -> EnrollResponse {
    reqwest::Client::new()
        .post(format!("{base}/v1/dev/enroll"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

struct Term {
    mcu: McuEmu,
    edge: EdgeAgent,
    dog_id: DogId,
}

impl Term {
    fn new(enroll: EnrollResponse, api: &str) -> Self {
        let dog_id = enroll.dog_id;
        let edge = EdgeAgent::new(EdgeConfig {
            api_base: api.into(),
            terminal_id: enroll.terminal_id,
            dog_id: enroll.dog_id,
            token: enroll.token,
            publish_ms: 2000,
        });
        Self {
            mcu: McuEmu::new(),
            edge,
            dog_id,
        }
    }

    async fn drive(&mut self, ticks: usize, publish_every: usize) {
        for i in 0..ticks {
            let bytes = self.mcu.tick_50ms();
            let snaps = self.edge.ingest_uart(&bytes, 50);
            let flip = snaps.iter().any(|s| s.flipped);
            if flip || (i + 1) % publish_every == 0 {
                self.edge.publish_now().await.expect("publish");
            }
        }
    }
}

#[tokio::test]
async fn dual_terminal_presence_and_leave() {
    let (addr, _api) = spawn_api().await;
    let base = format!("http://{addr}");

    let ea = enroll(&base).await;
    let eb = enroll(&base).await;
    let mut a = Term::new(ea, &base);
    let mut b = Term::new(eb, &base);

    a.drive(5, 5).await;
    b.drive(5, 5).await;

    let list: Vec<PresenceView> = reqwest::Client::new()
        .get(format!("{base}/v1/presence"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(list.is_empty(), "expected no one present yet");

    a.mcu.set_world(McuWorld::dog_on_mat(130.0));
    b.mcu.set_world(McuWorld::dog_on_mat(110.0));
    a.drive(30, 5).await;
    b.drive(30, 5).await;

    let list: Vec<PresenceView> = reqwest::Client::new()
        .get(format!("{base}/v1/presence"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list.len(), 2, "both dogs online: {list:?}");
    assert!(list.iter().all(|v| v.present));

    a.mcu.set_world(McuWorld::empty());
    a.drive(60, 5).await;
    b.drive(10, 5).await;

    let list: Vec<PresenceView> = reqwest::Client::new()
        .get(format!("{base}/v1/presence"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list.len(), 1, "only B remains: {list:?}");
    assert_eq!(list[0].dog_id, b.dog_id);
    assert_eq!(PRESENCE_TTL_MS, 10_000);
}

#[tokio::test]
async fn unauthorized_presence_rejected() {
    let (addr, _api) = spawn_api().await;
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();
    let res = client
        .post(format!("{base}/v1/presence"))
        .json(&serde_json::json!({
            "dog_id": uuid::Uuid::new_v4(),
            "terminal_id": uuid::Uuid::new_v4(),
            "present": true,
            "confidence": 1.0,
            "force_band": "medium",
            "force_n": 100.0,
            "tof_mm": 400,
            "ts": Utc::now(),
            "seq": 1
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
}
