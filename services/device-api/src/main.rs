//! Device API — presence + call invites.

use axum::{
    extract::{Path, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use clap::Parser;
use db::AppData;
use device_auth::SharedSecretAuthority;
use protocol::{
    AcceptInviteResponse, CreateInviteRequest, CreateInviteResponse, DogId, EndSessionRequest,
    IncomingInviteOffer, InviteId, LureConfig, PresenceReport, PresenceView, SessionId, TerminalId,
};
use serde::{Deserialize, Serialize};
use session::{accept_invite, new_invite, route_invite, webrtc_role, InviteError};
use std::sync::Arc;
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
}

#[derive(Clone)]
struct AppState {
    data: Arc<AppData>,
    auth: SharedSecretAuthority,
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
    let state = AppState {
        data: Arc::new(AppData::new()),
        auth: SharedSecretAuthority::new(args.device_secret),
    };

    let app = router(state);
    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    info!(%args.bind, "device-api listening");
    axum::serve(listener, app).await?;
    Ok(())
}

fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/healthz",
            get(|| async { Json(serde_json::json!({"ok": true})) }),
        )
        .route("/v1/presence", post(post_presence).get(list_presence))
        .route("/v1/presence/{dog_id}", get(get_presence))
        .route("/v1/dev/enroll", post(dev_enroll))
        .route("/v1/dev/bonds", post(dev_bootstrap_bond))
        .route("/v1/invites", post(create_invite))
        .route("/v1/invites/incoming", get(incoming_invite))
        .route("/v1/invites/{invite_id}/cancel", post(cancel_invite))
        .route(
            "/v1/invites/{invite_id}/accept",
            post(accept_invite_handler),
        )
        .route("/v1/sessions/active", get(active_session))
        .route("/v1/sessions/{session_id}/end", post(end_session))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct EnrollRequest {
    terminal_id: Option<Uuid>,
    dog_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
struct EnrollResponse {
    terminal_id: TerminalId,
    dog_id: DogId,
    token: String,
}

async fn dev_enroll(
    State(state): State<AppState>,
    Json(body): Json<EnrollRequest>,
) -> Json<EnrollResponse> {
    let terminal_id = TerminalId(body.terminal_id.unwrap_or_else(Uuid::new_v4));
    let dog_id = DogId(body.dog_id.unwrap_or_else(Uuid::new_v4));
    let id = state.auth.issue(terminal_id, dog_id);
    Json(EnrollResponse {
        terminal_id: id.terminal_id,
        dog_id: id.dog_id,
        token: id.token,
    })
}

#[derive(Debug, Deserialize)]
struct BondBootstrap {
    dog_a: Uuid,
    dog_b: Uuid,
    #[serde(default = "default_weight")]
    weight: f32,
}

fn default_weight() -> f32 {
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
    StatusCode::NO_CONTENT
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
    let terminal_id = TerminalId(Uuid::parse_str(tid).map_err(|_| StatusCode::UNAUTHORIZED)?);
    Ok((terminal_id, token.to_string()))
}

async fn post_presence(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(report): Json<PresenceReport>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    let (terminal_id, token) = extract_device_token(&headers).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "unauthorized".into(),
            }),
        )
    })?;
    if terminal_id != report.terminal_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                error: "terminal_id mismatch".into(),
            }),
        ));
    }
    state
        .auth
        .verify_pair(report.terminal_id, report.dog_id, &token)
        .map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorBody {
                    error: "invalid device token".into(),
                }),
            )
        })?;
    state.data.presence.upsert(report);
    // expire invites opportunistically
    state.data.invites.expire_due(Utc::now());
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

/// Caller identity is derived from device token + presence report dog binding.
/// Client sends Authorization and body `{ mode, to_dog?, dog_id, terminal_id }`.
#[derive(Debug, Deserialize)]
struct CreateInviteBody {
    #[serde(flatten)]
    req: CreateInviteRequest,
    dog_id: DogId,
    terminal_id: TerminalId,
}

async fn create_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateInviteBody>,
) -> Result<Json<CreateInviteResponse>, (StatusCode, Json<ErrorBody>)> {
    let (terminal_id, token) = extract_device_token(&headers).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "unauthorized".into(),
            }),
        )
    })?;
    if terminal_id != body.terminal_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                error: "terminal mismatch".into(),
            }),
        ));
    }
    state
        .auth
        .verify_pair(body.terminal_id, body.dog_id, &token)
        .map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorBody {
                    error: "invalid token".into(),
                }),
            )
        })?;

    let now = Utc::now();
    state.data.invites.expire_due(now);

    if state.data.invites.for_dog(body.dog_id).is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorBody {
                error: "caller busy".into(),
            }),
        ));
    }

    let present_ids = state.data.presence.present_dog_ids(now);
    let caller_present = state.data.presence.is_present(body.dog_id, now);
    let bonds = state.data.bonds.lock();
    let to = route_invite(
        body.dog_id,
        body.req.to_dog,
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
        (
            code,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    drop(bonds);

    if state.data.invites.for_dog(to).is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorBody {
                error: "peer busy".into(),
            }),
        ));
    }

    let mode = body.req.mode;
    let invite = new_invite(body.dog_id, to, mode);
    state.data.invites.insert(invite.clone()).map_err(|_| {
        (
            StatusCode::CONFLICT,
            Json(ErrorBody {
                error: "busy".into(),
            }),
        )
    })?;

    info!(
        from = %invite.from_dog,
        to = %invite.to_dog,
        id = %invite.id,
        "invite ringing"
    );
    Ok(Json(CreateInviteResponse { invite }))
}

#[derive(Debug, Deserialize)]
struct IncomingQuery {
    dog_id: Uuid,
    terminal_id: Uuid,
}

async fn incoming_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<IncomingQuery>,
) -> Result<Json<Option<IncomingInviteOffer>>, (StatusCode, Json<ErrorBody>)> {
    let (terminal_id, token) = extract_device_token(&headers).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "unauthorized".into(),
            }),
        )
    })?;
    let dog_id = DogId(q.dog_id);
    let tid = TerminalId(q.terminal_id);
    if tid != terminal_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                error: "terminal mismatch".into(),
            }),
        ));
    }
    state.auth.verify_pair(tid, dog_id, &token).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "invalid token".into(),
            }),
        )
    })?;

    state.data.invites.expire_due(Utc::now());
    let offer = state
        .data
        .invites
        .incoming_for(dog_id)
        .map(|invite| IncomingInviteOffer {
            invite,
            lure: LureConfig::default(),
        });
    Ok(Json(offer))
}

async fn cancel_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(invite_id): Path<Uuid>,
    Json(body): Json<serde_json::Value>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    let (terminal_id, token) = extract_device_token(&headers).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "unauthorized".into(),
            }),
        )
    })?;
    let dog_id = body
        .get("dog_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .map(DogId)
        .ok_or((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "dog_id required".into(),
            }),
        ))?;
    state
        .auth
        .verify_pair(terminal_id, dog_id, &token)
        .map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorBody {
                    error: "invalid token".into(),
                }),
            )
        })?;
    let id = protocol::InviteId(invite_id);
    if let Some(inv) = state.data.invites.get(id) {
        if inv.from_dog != dog_id && inv.to_dog != dog_id {
            return Err((
                StatusCode::FORBIDDEN,
                Json(ErrorBody {
                    error: "not party to invite".into(),
                }),
            ));
        }
        state.data.invites.close(id);
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "not found".into(),
            }),
        ))
    }
}

async fn accept_invite_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(invite_id): Path<Uuid>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<AcceptInviteResponse>, (StatusCode, Json<ErrorBody>)> {
    let (terminal_id, token) = extract_device_token(&headers).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "unauthorized".into(),
            }),
        )
    })?;
    let dog_id = body
        .get("dog_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .map(DogId)
        .ok_or((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "dog_id required".into(),
            }),
        ))?;
    let tid = body
        .get("terminal_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .map(TerminalId)
        .unwrap_or(terminal_id);
    if tid != terminal_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                error: "terminal mismatch".into(),
            }),
        ));
    }
    state
        .auth
        .verify_pair(terminal_id, dog_id, &token)
        .map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorBody {
                    error: "invalid token".into(),
                }),
            )
        })?;

    let now = Utc::now();
    state.data.invites.expire_due(now);
    let id = InviteId(invite_id);
    let invite = state.data.invites.get(id).ok_or((
        StatusCode::NOT_FOUND,
        Json(ErrorBody {
            error: "invite not found".into(),
        }),
    ))?;
    let present = state.data.presence.is_present(dog_id, now);
    let session = accept_invite(&invite, dog_id, present, now).map_err(|e| {
        (
            StatusCode::PRECONDITION_FAILED,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    state.data.invites.close(id);
    state.data.sessions.insert(session.clone()).map_err(|_| {
        (
            StatusCode::CONFLICT,
            Json(ErrorBody {
                error: "session busy".into(),
            }),
        )
    })?;
    let role = webrtc_role(&session, dog_id).to_string();
    info!(session = %session.id, dog = %dog_id, %role, "session active (media stub)");
    Ok(Json(AcceptInviteResponse {
        session,
        webrtc_role: role,
    }))
}

#[derive(Debug, Deserialize)]
struct ActiveQuery {
    dog_id: Uuid,
    terminal_id: Uuid,
}

async fn active_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<ActiveQuery>,
) -> Result<Json<Option<protocol::SessionRecord>>, (StatusCode, Json<ErrorBody>)> {
    let (terminal_id, token) = extract_device_token(&headers).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "unauthorized".into(),
            }),
        )
    })?;
    let dog_id = DogId(q.dog_id);
    let tid = TerminalId(q.terminal_id);
    if tid != terminal_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                error: "terminal mismatch".into(),
            }),
        ));
    }
    state.auth.verify_pair(tid, dog_id, &token).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "invalid token".into(),
            }),
        )
    })?;
    Ok(Json(state.data.sessions.for_dog(dog_id)))
}

async fn end_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<Uuid>,
    Json(body): Json<EndSessionRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorBody>)> {
    let (terminal_id, token) = extract_device_token(&headers).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "unauthorized".into(),
            }),
        )
    })?;
    if terminal_id != body.terminal_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                error: "terminal mismatch".into(),
            }),
        ));
    }
    state
        .auth
        .verify_pair(body.terminal_id, body.dog_id, &token)
        .map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorBody {
                    error: "invalid token".into(),
                }),
            )
        })?;
    let id = SessionId(session_id);
    let Some(sess) = state.data.sessions.get(id) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "not found".into(),
            }),
        ));
    };
    if sess.dog_a != body.dog_id && sess.dog_b != body.dog_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                error: "not party".into(),
            }),
        ));
    }
    state.data.sessions.end(id);
    info!(session = %id, reason = ?body.reason, "session ended");
    let _ = body.reason;
    Ok(StatusCode::NO_CONTENT)
}
