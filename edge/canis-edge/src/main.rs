//! Edge binary demo — synthetic mat bring-up.

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
        // Build synthetic sense frame via mcu path
        use mcu_emu::{McuEmu, McuWorld};
        // lightweight: use canis_sense via empty uart + direct - use publish from filter by driving sense
        // Directly tick presence through a local mcu frame
        let mut emu = McuEmu::new();
        if on_mat {
            emu.set_world(McuWorld::dog_on_mat(120.0));
        }
        let bytes = emu.tick_50ms();
        let (snaps, _) = agent.ingest_uart(&bytes, 100);
        for s in snaps {
            if s.flipped {
                info!(present = s.present, "presence flipped");
                agent.publish_now().await?;
            }
        }
        if elapsed % 2000 == 0 {
            agent.publish_now().await?;
        }
        elapsed += 100;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
