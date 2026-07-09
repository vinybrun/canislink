//! Steward API entrypoint — alpha proxies to device-api steward routes.
//! For production, split this service. For ship, run device-api which embeds steward.

use clap::Parser;
use tracing::info;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    device_api: String,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("steward_api=info")
        .init();
    let args = Args::parse();
    info!(
        device_api = %args.device_api,
        "steward-api alpha: use device-api /v1/steward/* with Authorization: Steward <secret>"
    );
}
