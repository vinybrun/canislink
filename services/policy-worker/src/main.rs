//! Policy worker — alpha documents that device-api runs an internal 1s policy tick.
//! This binary remains for deploy topology parity.

use clap::Parser;
use tracing::info;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    device_api: String,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("policy_worker=info")
        .init();
    let args = Args::parse();
    info!(
        device_api = %args.device_api,
        "policy-worker alpha: max-duration/segment/estop ticks run inside device-api"
    );
}
