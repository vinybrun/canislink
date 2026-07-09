//! Device API — presence, invites, sessions, config, media handshake, device WS.

mod events;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use events::{DeviceEvent, EventHub};
use futures_util::{SinkExt, StreamExt};
use chrono::Utc;
use clap::Parser;
use db::{sqlite, AppData};
use device_auth::SharedSecretAuthority;
use policy::{invite_rate_ok, social_allowed, PolicyDeny};
use protocol::{
    AcceptInviteResponse, AgainResponse, ConfigV1, CreateInviteResponse, DogId, EndSessionRequest,
    FeatureFlags, IceConfig, IncomingInviteOffer, InviteId, LureConfig, MediaReadyResponse, PadMap,
    PresenceReport, PresenceView, SessionId, SessionState, TerminalId,
};
use serde::{Deserialize, Serialize};
use session::{accept_invite, new_invite, route_invite, webrtc_role, InviteError};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::sync::Arc;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::info;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(name = "device-api")]
struct Args {
    #[arg(long, env = "CANIS_BIND", default_value = "0.0.0.0:8080")]
    bind: String,
    #[arg(long, env = "CANIS_DEVICE_SECRET", default_value = "canis-dev-secret")]
    device_secret: String,
    #[arg(
        long,
        env = "CANIS_STEWARD_SECRET",
        default_value = "canis-steward-secret"
    )]
    steward_secret: String,
    /// SQLite URL for durable lab state
    #[arg(
        long,
        env = "CANIS_DATABASE_URL",
        default_value = "sqlite:canislink.db?mode=rwc"
    )]
    database_url: String,
    /// Skip SQLite (CI / unit-style process tests)
    #[arg(long, env = "CANIS_EPHEMERAL", default_value_t = false)]
    ephemeral: bool,
}

#[derive(Clone)]
struct AppState {
    data: Arc<AppData>,
    auth: SharedSecretAuthority,
    steward_secret: String,
    pool: Option<SqlitePool>,
    events: EventHub,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "device_api=info,tower_http=info".into()),
        )
        .init();
    let args = Args::parse();
    let data = Arc::new(AppData::new());
    let pool = if args.ephemeral {
        info!("ephemeral mode — no SQLite durability");
        None
    } else {
        let pool = sqlite::open(&args.database_url).await?;
        sqlite::load_into(&pool, &data).await?;
        info!(db = %args.database_url, "durable SQLite enabled");
        Some(pool)
    };
    let state = AppState {
        data: data.clone(),
        auth: SharedSecretAuthority::new(args.device_secret),
        steward_secret: args.steward_secret,
        pool,
        events: EventHub::new(),
    };
    let tick_data = state.data.clone();
    let tick_pool = state.pool.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            policy_tick(&tick_data, tick_pool.as_ref()).await;
        }
    });
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    info!(%args.bind, "device-api listening (lab-durable)");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn policy_tick(data: &AppData, pool: Option<&SqlitePool>) {
    let now = Utc::now();
    for inv in data.invites.expire_due(now) {
        if let Some(p) = pool {
            let _ = sqlite::delete_invite(p, inv.id).await;
        }
    }
    for sess in data.sessions.all() {
        let pol_a = data.policies.get(sess.dog_a);
        let max_sec = pol_a.max_session_sec;
        let mut end = false;
        if (now - sess.started_at).num_seconds() as u64 >= max_sec {
            info!(session = %sess.id, "ended max duration");
            end = true;
        } else if now >= sess.segment_deadline_at && sess.state == SessionState::Active {
            info!(session = %sess.id, "ended segment expired");
            end = true;
        } else if data.policies.get(sess.dog_a).emergency_stop
            || data.policies.get(sess.dog_b).emergency_stop
        {
            info!(session = %sess.id, "ended emergency_stop");
            end = true;
        }
        if end {
            data.sessions.end(sess.id);
            if let Some(p) = pool {
                let _ = sqlite::delete_session(p, sess.id).await;
            }
        }
    }
}

fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/healthz",
            get(|| async { Json(serde_json::json!({"ok": true, "service": "device-api"})) }),
        )
        .route("/v1/presence", post(post_presence).get(list_presence))
        .route("/v1/presence/{dog_id}", get(get_presence))
        .route("/v1/dev/enroll", post(dev_enroll))
        .route("/v1/dev/bonds", post(dev_bootstrap_bond))
        .route("/v1/config", get(get_config))
        .route("/v1/ws", get(device_ws))
        .route("/v1/invites", post(create_invite))
        .route("/v1/invites/incoming", get(incoming_invite))
        .route("/v1/invites/{invite_id}/cancel", post(cancel_invite))
        .route(
            "/v1/invites/{invite_id}/accept",
            post(accept_invite_handler),
        )
        .route("/v1/sessions/active", get(active_session))
        .route("/v1/sessions/{session_id}/end", post(end_session))
        .route("/v1/sessions/{session_id}/media_ready", post(media_ready))
        .route("/v1/sessions/{session_id}/again", post(again))
        // steward routes hosted on same process for alpha single-binary ship
        .route("/v1/steward/estop", post(steward_estop))
        .route("/v1/steward/social_disabled", post(steward_social))
        .route("/v1/steward/bonds", post(steward_bonds))
        .route("/v1/steward/policy", post(steward_policy))
        .nest_service("/portal", ServeDir::new(std::env::var("CANIS_PORTAL_DIR").unwrap_or_else(|_| "portal-web".into())))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

fn extract_device_token(headers: &HeaderMap) -> Result<(TerminalId, String), StatusCode> {
    let raw = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let rest = raw
        .strip_prefix("Device ")
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let (tid, token) = rest.split_once(':').ok_or(StatusCode::UNAUTHORIZED)?;
    Ok((
        TerminalId(Uuid::parse_str(tid).map_err(|_| StatusCode::UNAUTHORIZED)?),
        token.into(),
    ))
}

fn verify_device(
    state: &AppState,
    headers: &HeaderMap,
    terminal_id: TerminalId,
    dog_id: DogId,
) -> Result<(), StatusCode> {
    let (tid, token) = extract_device_token(headers)?;
    if tid != terminal_id {
        return Err(StatusCode::FORBIDDEN);
    }
    state
        .auth
        .verify_pair(terminal_id, dog_id, &token)
        .map_err(|_| StatusCode::UNAUTHORIZED)
}

fn verify_steward(state: &AppState, headers: &HeaderMap) -> Result<(), StatusCode> {
    let raw = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let secret = raw
        .strip_prefix("Steward ")
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if secret == state.steward_secret {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<ErrorBody>) {
    (status, Json(ErrorBody { error: msg.into() }))
}

fn token_hash(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    hex::encode(h.finalize())
}

#[derive(Deserialize)]
struct EnrollRequest {
    terminal_id: Option<Uuid>,
    dog_id: Option<Uuid>,
}

async fn dev_enroll(
    State(state): State<AppState>,
    Json(body): Json<EnrollRequest>,
) -> Json<serde_json::Value> {
    let terminal_id = TerminalId(body.terminal_id.unwrap_or_else(Uuid::new_v4));
    let dog_id = DogId(body.dog_id.unwrap_or_else(Uuid::new_v4));
    let id = state.auth.issue(terminal_id, dog_id);
    state.data.policies.bind_terminal(terminal_id, dog_id);
    if let Some(pool) = &state.pool {
        let _ = sqlite::save_enroll(pool, terminal_id, dog_id, &token_hash(&id.token)).await;
    }
    Json(serde_json::json!({
        "terminal_id": id.terminal_id,
        "dog_id": id.dog_id,
        "token": id.token
    }))
}

#[derive(Deserialize)]
struct BondBootstrap {
    dog_a: Uuid,
    dog_b: Uuid,
    #[serde(default = "def_w")]
    weight: f32,
}
fn def_w() -> f32 {
    0.5
}

async fn dev_bootstrap_bond(
    State(state): State<AppState>,
    Json(body): Json<BondBootstrap>,
) -> StatusCode {
    state
        .data
        .bonds
        .lock()
        .bootstrap_mutual(DogId(body.dog_a), DogId(body.dog_b), body.weight);
    if let Some(pool) = &state.pool {
        let _ = sqlite::save_bond(pool, DogId(body.dog_a), DogId(body.dog_b), body.weight).await;
    }
    StatusCode::NO_CONTENT
}

async fn post_presence(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(report): Json<PresenceReport>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    verify_device(&state, &headers, report.terminal_id, report.dog_id)
        .map_err(|s| err(s, "unauthorized"))?;
    state
        .data
        .policies
        .bind_terminal(report.terminal_id, report.dog_id);
    if let Some(pool) = &state.pool {
        let _ = sqlite::save_presence(pool, &report).await;
    }
    state.data.presence.upsert(report);
    Ok(StatusCode::NO_CONTENT)
}

async fn get_presence(
    State(state): State<AppState>,
    Path(dog_id): Path<Uuid>,
) -> Result<Json<PresenceView>, StatusCode> {
    state
        .data
        .presence
        .get(DogId(dog_id), Utc::now())
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn list_presence(State(state): State<AppState>) -> Json<Vec<PresenceView>> {
    Json(state.data.presence.list_present(Utc::now()))
}

#[derive(Deserialize)]
struct DeviceQuery {
    dog_id: Uuid,
    terminal_id: Uuid,
}

async fn get_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<DeviceQuery>,
) -> Result<Json<ConfigV1>, (StatusCode, Json<ErrorBody>)> {
    let dog_id = DogId(q.dog_id);
    let terminal_id = TerminalId(q.terminal_id);
    verify_device(&state, &headers, terminal_id, dog_id).map_err(|s| err(s, "unauthorized"))?;
    let p = state.data.policies.get(dog_id);
    Ok(Json(ConfigV1 {
        dog_id,
        terminal_id,
        social_disabled: p.social_disabled,
        emergency_stop: p.emergency_stop,
        timezone: p.timezone,
        utc_offset_min: p.utc_offset_min,
        sleep_start_min: p.sleep_start_min,
        sleep_end_min: p.sleep_end_min,
        max_session_sec: p.max_session_sec,
        segment_sec: p.segment_sec,
        lure: LureConfig::default(),
        pad_map: PadMap::default(),
        ice: IceConfig::default(),
        features: FeatureFlags::default(),
    }))
}

#[derive(Deserialize)]
struct CreateInviteBody {
    mode: protocol::InviteMode,
    to_dog: Option<DogId>,
    dog_id: DogId,
    terminal_id: TerminalId,
}

#[axum::debug_handler]
async fn create_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateInviteBody>,
) -> Result<Json<CreateInviteResponse>, (StatusCode, Json<ErrorBody>)> {
    verify_device(&state, &headers, body.terminal_id, body.dog_id)
        .map_err(|s| err(s, "unauthorized"))?;
    let now = Utc::now();
    state.data.invites.expire_due(now);
    let pol = state.data.policies.get(body.dog_id);
    if let Err(e) = social_allowed(&pol, now) {
        let code = match e {
            PolicyDeny::EmergencyStop => StatusCode::from_u16(423).unwrap_or(StatusCode::FORBIDDEN),
            PolicyDeny::SocialDisabled => StatusCode::FORBIDDEN,
            PolicyDeny::Sleep => StatusCode::FORBIDDEN,
            PolicyDeny::RateLimit => StatusCode::TOO_MANY_REQUESTS,
        };
        return Err(err(code, e.to_string()));
    }
    let count = state.data.invites.invites_last_hour(body.dog_id, now);
    if invite_rate_ok(count, pol.max_invites_per_hour).is_err() {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "invite rate limit"));
    }
    if state.data.invites.for_dog(body.dog_id).is_some()
        || state.data.sessions.for_dog(body.dog_id).is_some()
    {
        return Err(err(StatusCode::CONFLICT, "caller busy"));
    }
    let present_ids = state.data.presence.present_dog_ids(now);
    let caller_present = state.data.presence.is_present(body.dog_id, now);
    let to = {
        let bonds = state.data.bonds.lock();
        route_invite(
            body.dog_id,
            body.to_dog,
            &bonds,
            &present_ids,
            caller_present,
        )
        .map_err(|e| {
            let code = match e {
                InviteError::CallerNotPresent => StatusCode::PRECONDITION_FAILED,
                InviteError::NoEligiblePeer | InviteError::PeerNotPresent => StatusCode::NOT_FOUND,
                InviteError::NotBonded => StatusCode::FORBIDDEN,
                InviteError::CallerBusy | InviteError::PeerBusy => StatusCode::CONFLICT,
            };
            err(code, e.to_string())
        })?
    };
    let peer_pol = state.data.policies.get(to);
    if let Err(e) = social_allowed(&peer_pol, now) {
        return Err(err(StatusCode::FORBIDDEN, format!("peer: {e}")));
    }
    if state.data.invites.for_dog(to).is_some() || state.data.sessions.for_dog(to).is_some() {
        return Err(err(StatusCode::CONFLICT, "peer busy"));
    }
    let invite = new_invite(body.dog_id, to, body.mode);
    state
        .data
        .invites
        .insert(invite.clone())
        .map_err(|_| err(StatusCode::CONFLICT, "busy"))?;
    state.data.invites.record_invite(body.dog_id, now);
    if let Some(pool) = &state.pool {
        let _ = sqlite::save_invite(pool, &invite).await;
        let _ = sqlite::record_invite_event(pool, body.dog_id, now).await;
    }
    info!(from = %invite.from_dog, to = %invite.to_dog, id = %invite.id, "invite ringing");
    state
        .events
        .publish(
            invite.to_dog,
            DeviceEvent::InviteRinging {
                invite: invite.clone(),
                lure_led: "slow_pulse_blue".into(),
            },
        )
        .await;
    Ok(Json(CreateInviteResponse { invite }))
}

async fn incoming_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<DeviceQuery>,
) -> Result<Json<Option<IncomingInviteOffer>>, (StatusCode, Json<ErrorBody>)> {
    let dog_id = DogId(q.dog_id);
    let terminal_id = TerminalId(q.terminal_id);
    verify_device(&state, &headers, terminal_id, dog_id).map_err(|s| err(s, "unauthorized"))?;
    let pol = state.data.policies.get(dog_id);
    if pol.emergency_stop || pol.social_disabled {
        return Ok(Json(None));
    }
    state.data.invites.expire_due(Utc::now());
    Ok(Json(state.data.invites.incoming_for(dog_id).map(
        |invite| IncomingInviteOffer {
            invite,
            lure: LureConfig::default(),
        },
    )))
}

async fn cancel_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(invite_id): Path<Uuid>,
    Json(body): Json<serde_json::Value>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    let dog_id = DogId(
        Uuid::parse_str(body["dog_id"].as_str().unwrap_or_default())
            .map_err(|_| err(StatusCode::BAD_REQUEST, "dog_id"))?,
    );
    let (tid, token) = extract_device_token(&headers).map_err(|s| err(s, "unauthorized"))?;
    state
        .auth
        .verify_pair(tid, dog_id, &token)
        .map_err(|_| err(StatusCode::UNAUTHORIZED, "invalid token"))?;
    let id = InviteId(invite_id);
    if let Some(inv) = state.data.invites.get(id) {
        if inv.from_dog != dog_id && inv.to_dog != dog_id {
            return Err(err(StatusCode::FORBIDDEN, "not party"));
        }
        state.data.invites.close(id);
        if let Some(pool) = &state.pool {
            let _ = sqlite::delete_invite(pool, id).await;
        }
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(err(StatusCode::NOT_FOUND, "not found"))
    }
}

async fn accept_invite_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(invite_id): Path<Uuid>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<AcceptInviteResponse>, (StatusCode, Json<ErrorBody>)> {
    let dog_id = DogId(
        Uuid::parse_str(body["dog_id"].as_str().unwrap_or_default())
            .map_err(|_| err(StatusCode::BAD_REQUEST, "dog_id"))?,
    );
    let terminal_id = TerminalId(
        Uuid::parse_str(body["terminal_id"].as_str().unwrap_or_default())
            .map_err(|_| err(StatusCode::BAD_REQUEST, "terminal_id"))?,
    );
    verify_device(&state, &headers, terminal_id, dog_id).map_err(|s| err(s, "unauthorized"))?;
    let now = Utc::now();
    let pol = state.data.policies.get(dog_id);
    social_allowed(&pol, now).map_err(|e| err(StatusCode::FORBIDDEN, e.to_string()))?;
    state.data.invites.expire_due(now);
    let id = InviteId(invite_id);
    let invite = state
        .data
        .invites
        .get(id)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "invite not found"))?;
    let present = state.data.presence.is_present(dog_id, now);
    let mut session = accept_invite(&invite, dog_id, present, now)
        .map_err(|e| err(StatusCode::PRECONDITION_FAILED, e.to_string()))?;
    // start as Negotiating until both media_ready (alpha may mark ready immediately via edge)
    session.state = SessionState::Negotiating;
    // apply policy max/segment
    let max_sec = pol.max_session_sec as i64;
    let seg_sec = pol.segment_sec as i64;
    session.max_end_at = now + chrono::Duration::seconds(max_sec);
    session.segment_deadline_at = now + chrono::Duration::seconds(seg_sec);
    state.data.invites.close(id);
    state
        .data
        .sessions
        .insert(session.clone())
        .map_err(|_| err(StatusCode::CONFLICT, "session busy"))?;
    if let Some(pool) = &state.pool {
        let _ = sqlite::delete_invite(pool, id).await;
        let _ = sqlite::save_session(pool, &session, false, false).await;
    }
    let role = webrtc_role(&session, dog_id).to_string();
    info!(session = %session.id, dog = %dog_id, %role, "session negotiating");
    state
        .events
        .publish_many(
            &[session.dog_a, session.dog_b],
            DeviceEvent::SessionUpdated {
                session: session.clone(),
            },
        )
        .await;
    Ok(Json(AcceptInviteResponse {
        session,
        webrtc_role: role,
    }))
}

async fn active_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<DeviceQuery>,
) -> Result<Json<Option<protocol::SessionRecord>>, (StatusCode, Json<ErrorBody>)> {
    let dog_id = DogId(q.dog_id);
    let terminal_id = TerminalId(q.terminal_id);
    verify_device(&state, &headers, terminal_id, dog_id).map_err(|s| err(s, "unauthorized"))?;
    Ok(Json(state.data.sessions.for_dog(dog_id)))
}

async fn end_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<Uuid>,
    Json(body): Json<EndSessionRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    verify_device(&state, &headers, body.terminal_id, body.dog_id)
        .map_err(|s| err(s, "unauthorized"))?;
    let id = SessionId(session_id);
    let Some(sess) = state.data.sessions.get(id) else {
        return Err(err(StatusCode::NOT_FOUND, "not found"));
    };
    if sess.dog_a != body.dog_id && sess.dog_b != body.dog_id {
        return Err(err(StatusCode::FORBIDDEN, "not party"));
    }
    if let Some(sess) = state.data.sessions.get(id) {
        state
            .events
            .publish_many(
                &[sess.dog_a, sess.dog_b],
                DeviceEvent::SessionEnded {
                    session_id: id.to_string(),
                    reason: format!("{:?}", body.reason),
                },
            )
            .await;
    }
    state.data.sessions.end(id);
    if let Some(pool) = &state.pool {
        let _ = sqlite::delete_session(pool, id).await;
    }
    info!(session = %id, reason = ?body.reason, "session ended");
    Ok(StatusCode::NO_CONTENT)
}

async fn media_ready(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<Uuid>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<MediaReadyResponse>, (StatusCode, Json<ErrorBody>)> {
    let dog_id = DogId(
        Uuid::parse_str(body["dog_id"].as_str().unwrap_or_default())
            .map_err(|_| err(StatusCode::BAD_REQUEST, "dog_id"))?,
    );
    let terminal_id = TerminalId(
        Uuid::parse_str(body["terminal_id"].as_str().unwrap_or_default())
            .map_err(|_| err(StatusCode::BAD_REQUEST, "terminal_id"))?,
    );
    verify_device(&state, &headers, terminal_id, dog_id).map_err(|s| err(s, "unauthorized"))?;
    let ready = body.get("ready").and_then(|v| v.as_bool()).unwrap_or(true);
    let id = SessionId(session_id);
    let (both, session) = state
        .data
        .sessions
        .set_media_ready(id, dog_id, ready)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "session"))?;
    if let Some(pool) = &state.pool {
        let ma = dog_id == session.dog_a && ready || both;
        let mb = dog_id == session.dog_b && ready || both;
        // store both flags as both when both; approximate: re-set both true if both
        let _ = sqlite::save_session(
            pool,
            &session,
            both || dog_id == session.dog_a,
            both || dog_id == session.dog_b,
        )
        .await;
        let _ = (ma, mb);
    }
    if both {
        info!(session = %id, "both media ready → Active");
    }
    Ok(Json(MediaReadyResponse {
        both_ready: both,
        session,
    }))
}

async fn again(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<Uuid>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<AgainResponse>, (StatusCode, Json<ErrorBody>)> {
    let dog_id = DogId(
        Uuid::parse_str(body["dog_id"].as_str().unwrap_or_default())
            .map_err(|_| err(StatusCode::BAD_REQUEST, "dog_id"))?,
    );
    let terminal_id = TerminalId(
        Uuid::parse_str(body["terminal_id"].as_str().unwrap_or_default())
            .map_err(|_| err(StatusCode::BAD_REQUEST, "terminal_id"))?,
    );
    verify_device(&state, &headers, terminal_id, dog_id).map_err(|s| err(s, "unauthorized"))?;
    let id = SessionId(session_id);
    let sess = state
        .data
        .sessions
        .get(id)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "session"))?;
    if sess.dog_a != dog_id && sess.dog_b != dog_id {
        return Err(err(StatusCode::FORBIDDEN, "not party"));
    }
    let pol = state.data.policies.get(dog_id);
    let now = Utc::now();
    let new_deadline = policy::extend_segment(now, pol.segment_sec, sess.max_end_at);
    let session = state
        .data
        .sessions
        .update(id, |s| s.segment_deadline_at = new_deadline)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "session"))?;
    if let Some(pool) = &state.pool {
        let _ = sqlite::save_session(pool, &session, true, true).await;
    }
    info!(session = %id, until = %new_deadline, "segment extended");
    Ok(Json(AgainResponse { session }))
}

// --- Steward ---

#[derive(Deserialize)]
struct StewardDogFlag {
    dog_id: Uuid,
    enabled: bool,
}

async fn steward_estop(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<StewardDogFlag>,
) -> Result<StatusCode, StatusCode> {
    verify_steward(&state, &headers)?;
    state
        .data
        .policies
        .set_estop(DogId(body.dog_id), body.enabled);
    if let Some(pool) = &state.pool {
        let p = state.data.policies.get(DogId(body.dog_id));
        let _ = sqlite::save_policy(pool, &p).await;
    }
    if body.enabled {
        if let Some(s) = state.data.sessions.for_dog(DogId(body.dog_id)) {
            state.data.sessions.end(s.id);
            if let Some(pool) = &state.pool {
                let _ = sqlite::delete_session(pool, s.id).await;
            }
        }
    }
    info!(dog = %body.dog_id, enabled = body.enabled, "steward emergency_stop");
    Ok(StatusCode::NO_CONTENT)
}

async fn steward_social(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<StewardDogFlag>,
) -> Result<StatusCode, StatusCode> {
    verify_steward(&state, &headers)?;
    state
        .data
        .policies
        .set_social_disabled(DogId(body.dog_id), body.enabled);
    if let Some(pool) = &state.pool {
        let p = state.data.policies.get(DogId(body.dog_id));
        let _ = sqlite::save_policy(pool, &p).await;
    }
    info!(dog = %body.dog_id, enabled = body.enabled, "steward social_disabled");
    Ok(StatusCode::NO_CONTENT)
}

async fn steward_bonds(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<BondBootstrap>,
) -> Result<StatusCode, StatusCode> {
    verify_steward(&state, &headers)?;
    state
        .data
        .bonds
        .lock()
        .bootstrap_mutual(DogId(body.dog_a), DogId(body.dog_b), body.weight);
    if let Some(pool) = &state.pool {
        let _ = sqlite::save_bond(pool, DogId(body.dog_a), DogId(body.dog_b), body.weight).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct StewardPolicyBody {
    dog_id: Uuid,
    sleep_start_min: Option<u16>,
    sleep_end_min: Option<u16>,
    utc_offset_min: Option<i16>,
    max_session_sec: Option<u64>,
    segment_sec: Option<u64>,
    max_invites_per_hour: Option<u32>,
}

async fn steward_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<StewardPolicyBody>,
) -> Result<StatusCode, StatusCode> {
    verify_steward(&state, &headers)?;
    let dog = DogId(body.dog_id);
    let mut p = state.data.policies.get(dog);
    if let Some(v) = body.sleep_start_min {
        p.sleep_start_min = v;
    }
    if let Some(v) = body.sleep_end_min {
        p.sleep_end_min = v;
    }
    if let Some(v) = body.utc_offset_min {
        p.utc_offset_min = v;
    }
    if let Some(v) = body.max_session_sec {
        p.max_session_sec = v;
    }
    if let Some(v) = body.segment_sec {
        p.segment_sec = v;
    }
    if let Some(v) = body.max_invites_per_hour {
        p.max_invites_per_hour = v;
    }
    if let Some(pool) = &state.pool {
        let _ = sqlite::save_policy(pool, &p).await;
    }
    state.data.policies.set(p);
    Ok(StatusCode::NO_CONTENT)
}


/// Device realtime channel: `GET /v1/ws?dog_id=&terminal_id=` with Device auth header.
/// Query also accepts `token=` for WebView clients that cannot set WS headers easily.
async fn device_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    let dog_id = DogId(
        Uuid::parse_str(q.get("dog_id").ok_or(StatusCode::BAD_REQUEST)?)
            .map_err(|_| StatusCode::BAD_REQUEST)?,
    );
    let terminal_id = TerminalId(
        Uuid::parse_str(q.get("terminal_id").ok_or(StatusCode::BAD_REQUEST)?)
            .map_err(|_| StatusCode::BAD_REQUEST)?,
    );
    let token = if let Ok((_, t)) = extract_device_token(&headers) {
        t
    } else {
        q.get("token").cloned().ok_or(StatusCode::UNAUTHORIZED)?
    };
    state
        .auth
        .verify_pair(terminal_id, dog_id, &token)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    Ok(ws.on_upgrade(move |socket| device_ws_loop(socket, state, dog_id)))
}

async fn device_ws_loop(socket: WebSocket, state: AppState, dog_id: DogId) {
    let mut rx = state.events.subscribe(dog_id).await;
    let (mut sink, mut stream) = socket.split();
    // greet
    let hello = serde_json::json!({"event": "hello", "dog_id": dog_id});
    let _ = sink
        .send(Message::Text(hello.to_string().into()))
        .await;

    let mut send_task = tokio::spawn(async move {
        while let Ok(ev) = rx.recv().await {
            if let Ok(text) = serde_json::to_string(&ev) {
                if sink.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Text(t) if t.contains("ping") => {}
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}
