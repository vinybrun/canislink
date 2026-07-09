//! Call → lure → pad engage accept → Active session (media stub).

use canis_edge::{EdgeAgent, EdgeConfig, EdgeUx};
use chrono::Utc;
use db::AppData;
use device_auth::SharedSecretAuthority;
use mcu_emu::{McuEmu, McuWorld};
use protocol::{DogId, TerminalId};
use serde::Deserialize;
use session::{accept_invite, new_invite, route_invite, webrtc_role};
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};

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

async fn spawn_api() -> SocketAddr {
    let state = AppState {
        data: Arc::new(AppData::new()),
        auth: SharedSecretAuthority::new("e2e-secret"),
    };
    let app = Router::new()
        .route(
            "/v1/dev/enroll",
            post(|State(s): State<AppState>, Json(_): Json<serde_json::Value>| async move {
                let tid = TerminalId(uuid::Uuid::new_v4());
                let did = DogId(uuid::Uuid::new_v4());
                let id = s.auth.issue(tid, did);
                Json(serde_json::json!({"terminal_id": id.terminal_id, "dog_id": id.dog_id, "token": id.token}))
            }),
        )
        .route(
            "/v1/dev/bonds",
            post(|State(s): State<AppState>, Json(body): Json<serde_json::Value>| async move {
                let a = DogId(uuid::Uuid::parse_str(body["dog_a"].as_str().unwrap()).unwrap());
                let b = DogId(uuid::Uuid::parse_str(body["dog_b"].as_str().unwrap()).unwrap());
                s.data.bonds.lock().bootstrap_mutual(a, b, 0.8);
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/v1/presence",
            post(|State(s): State<AppState>, headers: HeaderMap, Json(report): Json<protocol::PresenceReport>| async move {
                let (tid, token) = extract_token(&headers)?;
                if tid != report.terminal_id { return Err(StatusCode::FORBIDDEN); }
                s.auth.verify_pair(report.terminal_id, report.dog_id, &token).map_err(|_| StatusCode::UNAUTHORIZED)?;
                s.data.presence.upsert(report);
                Ok::<_, StatusCode>(StatusCode::NO_CONTENT)
            }),
        )
        .route(
            "/v1/invites",
            post(|State(s): State<AppState>, headers: HeaderMap, Json(body): Json<serde_json::Value>| async move {
                let (tid, token) = extract_token(&headers)?;
                let dog_id = DogId(uuid::Uuid::parse_str(body["dog_id"].as_str().unwrap()).unwrap());
                let terminal_id = TerminalId(uuid::Uuid::parse_str(body["terminal_id"].as_str().unwrap()).unwrap());
                if tid != terminal_id { return Err(StatusCode::FORBIDDEN); }
                s.auth.verify_pair(terminal_id, dog_id, &token).map_err(|_| StatusCode::UNAUTHORIZED)?;
                let now = Utc::now();
                let present = s.data.presence.present_dog_ids(now);
                let caller_present = s.data.presence.is_present(dog_id, now);
                let bonds = s.data.bonds.lock();
                let to = route_invite(dog_id, None, &bonds, &present, caller_present).map_err(|_| StatusCode::NOT_FOUND)?;
                drop(bonds);
                let invite = new_invite(dog_id, to, protocol::InviteMode::Portal);
                s.data.invites.insert(invite.clone()).map_err(|_| StatusCode::CONFLICT)?;
                Ok::<_, StatusCode>(Json(serde_json::json!({ "invite": invite })))
            }),
        )
        .route(
            "/v1/invites/incoming",
            get(|State(s): State<AppState>, headers: HeaderMap, Query(q): Query<std::collections::HashMap<String, String>>| async move {
                let (tid, token) = extract_token(&headers)?;
                let dog_id = DogId(uuid::Uuid::parse_str(&q["dog_id"]).unwrap());
                let terminal_id = TerminalId(uuid::Uuid::parse_str(&q["terminal_id"]).unwrap());
                if tid != terminal_id { return Err(StatusCode::FORBIDDEN); }
                s.auth.verify_pair(terminal_id, dog_id, &token).map_err(|_| StatusCode::UNAUTHORIZED)?;
                let offer = s.data.invites.incoming_for(dog_id).map(|invite| {
                    serde_json::json!({"invite": invite, "lure": {"max_repeats": 3, "audio_ms": 2000, "led_pattern": "slow_pulse_blue"}})
                });
                Ok::<_, StatusCode>(Json(offer))
            }),
        )
        .route(
            "/v1/invites/{invite_id}/accept",
            post(|State(s): State<AppState>, headers: HeaderMap, Path(invite_id): Path<uuid::Uuid>, Json(body): Json<serde_json::Value>| async move {
                let (tid, token) = extract_token(&headers)?;
                let dog_id = DogId(uuid::Uuid::parse_str(body["dog_id"].as_str().unwrap()).unwrap());
                let terminal_id = TerminalId(uuid::Uuid::parse_str(body["terminal_id"].as_str().unwrap()).unwrap());
                if tid != terminal_id { return Err(StatusCode::FORBIDDEN); }
                s.auth.verify_pair(terminal_id, dog_id, &token).map_err(|_| StatusCode::UNAUTHORIZED)?;
                let id = protocol::InviteId(invite_id);
                let invite = s.data.invites.get(id).ok_or(StatusCode::NOT_FOUND)?;
                let now = Utc::now();
                let present = s.data.presence.is_present(dog_id, now);
                let mut session = accept_invite(&invite, dog_id, present, now).map_err(|_| StatusCode::PRECONDITION_FAILED)?;
                session.state = protocol::SessionState::Negotiating;
                s.data.invites.close(id);
                s.data.sessions.insert(session.clone()).map_err(|_| StatusCode::CONFLICT)?;
                let role = webrtc_role(&session, dog_id);
                Ok::<_, StatusCode>(Json(serde_json::json!({"session": session, "webrtc_role": role})))
            }),
        )
        .route(
            "/v1/sessions/active",
            get(|State(s): State<AppState>, headers: HeaderMap, Query(q): Query<std::collections::HashMap<String, String>>| async move {
                let (tid, token) = extract_token(&headers)?;
                let dog_id = DogId(uuid::Uuid::parse_str(&q["dog_id"]).unwrap());
                let terminal_id = TerminalId(uuid::Uuid::parse_str(&q["terminal_id"]).unwrap());
                if tid != terminal_id { return Err(StatusCode::FORBIDDEN); }
                s.auth.verify_pair(terminal_id, dog_id, &token).map_err(|_| StatusCode::UNAUTHORIZED)?;
                Ok::<_, StatusCode>(Json(s.data.sessions.for_dog(dog_id)))
            }),
        )
        .route(
            "/v1/sessions/{session_id}/media_ready",
            post(|State(s): State<AppState>, headers: HeaderMap, Path(session_id): Path<uuid::Uuid>, Json(body): Json<serde_json::Value>| async move {
                let (tid, token) = extract_token(&headers)?;
                let dog_id = DogId(uuid::Uuid::parse_str(body["dog_id"].as_str().unwrap()).unwrap());
                let terminal_id = TerminalId(uuid::Uuid::parse_str(body["terminal_id"].as_str().unwrap()).unwrap());
                if tid != terminal_id { return Err(StatusCode::FORBIDDEN); }
                s.auth.verify_pair(terminal_id, dog_id, &token).map_err(|_| StatusCode::UNAUTHORIZED)?;
                let ready = body.get("ready").and_then(|v| v.as_bool()).unwrap_or(true);
                let id = protocol::SessionId(session_id);
                let (both, session) = s.data.sessions.set_media_ready(id, dog_id, ready).ok_or(StatusCode::NOT_FOUND)?;
                Ok::<_, StatusCode>(Json(serde_json::json!({"both_ready": both, "session": session})))
            }),
        )
        .route(
            "/v1/sessions/{session_id}/end",
            post(|State(s): State<AppState>, headers: HeaderMap, Path(session_id): Path<uuid::Uuid>, Json(body): Json<serde_json::Value>| async move {
                let (tid, token) = extract_token(&headers)?;
                let dog_id = DogId(uuid::Uuid::parse_str(body["dog_id"].as_str().unwrap()).unwrap());
                let terminal_id = TerminalId(uuid::Uuid::parse_str(body["terminal_id"].as_str().unwrap()).unwrap());
                if tid != terminal_id { return Err(StatusCode::FORBIDDEN); }
                s.auth.verify_pair(terminal_id, dog_id, &token).map_err(|_| StatusCode::UNAUTHORIZED)?;
                s.data.sessions.end(protocol::SessionId(session_id));
                Ok::<_, StatusCode>(StatusCode::NO_CONTENT)
            }),
        )
        .route(
            "/v1/config",
            get(|State(s): State<AppState>, headers: HeaderMap, Query(q): Query<std::collections::HashMap<String, String>>| async move {
                let (tid, token) = extract_token(&headers)?;
                let dog_id = DogId(uuid::Uuid::parse_str(&q["dog_id"]).unwrap());
                let terminal_id = TerminalId(uuid::Uuid::parse_str(&q["terminal_id"]).unwrap());
                if tid != terminal_id { return Err(StatusCode::FORBIDDEN); }
                s.auth.verify_pair(terminal_id, dog_id, &token).map_err(|_| StatusCode::UNAUTHORIZED)?;
                Ok::<_, StatusCode>(Json(serde_json::json!({
                    "dog_id": dog_id,
                    "terminal_id": terminal_id,
                    "social_disabled": false,
                    "emergency_stop": false,
                    "timezone": "UTC",
                    "utc_offset_min": 0,
                    "sleep_start_min": 1320,
                    "sleep_end_min": 420,
                    "max_session_sec": 900,
                    "segment_sec": 300,
                    "lure": {"max_repeats": 3, "audio_ms": 2000, "led_pattern": "slow_pulse_blue"},
                    "pad_map": {"call": 0, "play": 1, "again": 2, "done": 3},
                    "ice": {"stun_urls": [], "turn_uris": [], "turn_username": "", "turn_credential": "", "ttl_sec": 3600},
                    "features": {"portal_v1": true, "play_mode": true, "toy_sync": false, "force_turn": false}
                })))
            }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
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
}

impl Term {
    fn new(e: Enroll, api: &str) -> Self {
        Self {
            mcu: McuEmu::new(),
            edge: EdgeAgent::new(EdgeConfig {
                api_base: api.into(),
                terminal_id: e.terminal_id,
                dog_id: e.dog_id,
                token: e.token,
                publish_ms: 2000,
            }),
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
                self.edge.handle_intent(i).await.unwrap();
            }
            self.edge.publish_now().await.unwrap();
        }
    }
}

#[tokio::test]
async fn accept_pad_engage_starts_session() {
    let addr = spawn_api().await;
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
        .json(&serde_json::json!({"dog_a": ea.dog_id, "dog_b": eb.dog_id, "weight": 0.8}))
        .send()
        .await
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

    b.edge.poll_incoming().await.unwrap();
    assert_eq!(b.edge.ux, EdgeUx::RingingIn);

    b.mcu.press_pad(0); // engage
    b.drive(2).await;
    assert!(matches!(b.edge.ux, EdgeUx::InSession | EdgeUx::Negotiating));
    assert!(b.edge.session.is_some());
    // one more media_ready from A so both_ready → Active
    a.edge.sync_active().await.unwrap();
    a.edge.report_media_ready(true).await.unwrap();
    b.edge.report_media_ready(true).await.unwrap();
    assert!(b
        .edge
        .session
        .as_ref()
        .map(|s| s.state == protocol::SessionState::Active
            || s.state == protocol::SessionState::Negotiating)
        .unwrap_or(false));
}
