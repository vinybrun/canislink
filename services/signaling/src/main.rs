//! Session-scoped WebRTC signaling (SDP/ICE relay).
//! Lab-shippable: peers join a room by session_id and exchange SignalMsg JSON.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use clap::Parser;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use media_signal::SignalMsg;
use protocol::SessionId;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, env = "CANIS_SIGNAL_BIND", default_value = "0.0.0.0:8081")]
    bind: String,
}

#[derive(Clone)]
struct App {
    rooms: Arc<DashMap<SessionId, broadcast::Sender<String>>>,
}

impl App {
    fn room_tx(&self, session: SessionId) -> broadcast::Sender<String> {
        self.rooms
            .entry(session)
            .or_insert_with(|| broadcast::channel(64).0)
            .clone()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "signaling=info".into()),
        )
        .init();
    let args = Args::parse();
    let app_state = App {
        rooms: Arc::new(DashMap::new()),
    };
    let app = Router::new()
        .route(
            "/healthz",
            get(|| async {
                Json(serde_json::json!({
                    "ok": true,
                    "service": "signaling",
                    "webrtc": "sdp_ice_relay"
                }))
            }),
        )
        .route("/v1/signal/{session_id}", get(ws_handler))
        .with_state(app_state);
    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    info!(%args.bind, "signaling listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(session_id): Path<Uuid>,
    State(app): State<App>,
) -> impl IntoResponse {
    let session = SessionId(session_id);
    ws.on_upgrade(move |socket| peer_loop(socket, session, app))
}

async fn peer_loop(socket: WebSocket, session: SessionId, app: App) {
    let tx = app.room_tx(session);
    let mut rx = tx.subscribe();
    let (mut sink, mut stream) = socket.split();

    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sink.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let tx2 = tx.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Text(text) => {
                    // validate JSON shape loosely
                    if serde_json::from_str::<SignalMsg>(&text).is_err() {
                        warn!(%session, "invalid signal json");
                        continue;
                    }
                    let _ = tx2.send(text.to_string());
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
    info!(%session, "peer left signal room");
}
