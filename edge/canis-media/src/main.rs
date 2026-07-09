//! canis-media — alpha media plane stub.
//!
//! Production will host GStreamer WebRTC. Alpha declares readiness to device-api
//! via canis-edge; this binary exists for process-model parity and health.

use clap::Parser;
use tracing::info;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "loopback")]
    mode: String,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("canis_media=info")
        .init();
    let args = Args::parse();
    info!(mode = %args.mode, "canis-media stub online (WebRTC deferred; control plane ready)");
    // stay alive for process supervisors
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
