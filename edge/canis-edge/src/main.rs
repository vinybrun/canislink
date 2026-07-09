//! Edge binary — demo loop with synthetic mat (for local bring-up).
//! Production reads UART via canis-sense; sim-dog drives EdgeAgent in-process.

use canis_edge::{EdgeAgent, EdgeConfig};
use clap::Parser;
use protocol::{DogId, TerminalId};
use tracing::info;
use uuid::Uuid;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, env = "CANIS_API", default_value = "http://127.0.0.1:8080")]
    api: String,
    #[arg(long)]
    terminal_id: Uuid,
    #[arg(long)]
    dog_id: Uuid,
    #[arg(long)]
    token: String,
    /// Simulate dog on mat after N ms.
    #[arg(long, default_value_t = 1000)]
    step_on_ms: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("canis_edge=info")
        .init();
    let args = Args::parse();
    let mut agent = EdgeAgent::new(EdgeConfig {
        api_base: args.api,
        terminal_id: TerminalId(args.terminal_id),
        dog_id: DogId(args.dog_id),
        token: args.token,
        publish_ms: 2000,
    });

    let mut elapsed = 0u64;
    loop {
        let on_mat = elapsed >= args.step_on_ms;
        let force = if on_mat { 120.0 } else { 0.0 };
        let tof = if on_mat { Some(400) } else { Some(2000) };
        let snap = agent.ingest_sample(force, tof, on_mat, 100);
        if snap.flipped {
            info!(present = snap.present, "local presence flipped");
            agent.publish_now().await?;
        } else if elapsed % 2000 == 0 {
            agent.publish_now().await?;
        }
        elapsed += 100;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
