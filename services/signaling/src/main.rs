//! WebRTC signaling room service (alpha scaffold).
//! Real SDP/ICE relay lands with GStreamer media. Health endpoint for compose.

use axum::{routing::get, Json, Router};
use clap::Parser;
use tracing::info;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "0.0.0.0:8081")]
    bind: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("signaling=info")
        .init();
    let args = Args::parse();
    let app = Router::new().route(
        "/healthz",
        get(|| async {
            Json(serde_json::json!({"ok": true, "service": "signaling", "webrtc": "pending"}))
        }),
    );
    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    info!(%args.bind, "signaling listening");
    axum::serve(listener, app).await?;
    Ok(())
}
