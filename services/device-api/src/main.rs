//! Device API — presence heartbeats (Feature: Presence).

use axum::{
    extract::{Path, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use clap::Parser;
use db::PresenceStore;
use device_auth::SharedSecretAuthority;
use protocol::{DogId, PresenceReport, PresenceView, TerminalId};
use serde::{Deserialize, Serialize};
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
    presence: PresenceStore,
    auth: SharedSecretAuthority,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Debug, Serialize)]
struct Health {
    ok: bool,
    service: &'static str,
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
        presence: PresenceStore::new(),
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
        .route("/healthz", get(healthz))
        .route("/v1/presence", post(post_presence).get(list_presence))
        .route("/v1/presence/{dog_id}", get(get_presence))
        .route("/v1/dev/enroll", post(dev_enroll))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn healthz() -> Json<Health> {
    Json(Health {
        ok: true,
        service: "device-api",
    })
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

/// Lab-only enrollment (shared secret issuer). Production uses mTLS provision.
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

    state.presence.upsert(report);
    Ok(StatusCode::NO_CONTENT)
}

async fn get_presence(
    State(state): State<AppState>,
    Path(dog_id): Path<Uuid>,
) -> Result<Json<PresenceView>, StatusCode> {
    let dog = DogId(dog_id);
    state
        .presence
        .get(dog, Utc::now())
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn list_presence(State(state): State<AppState>) -> Json<Vec<PresenceView>> {
    Json(state.presence.list_present(Utc::now()))
}
