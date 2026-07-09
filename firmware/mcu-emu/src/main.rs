//! Write UART stream to a path (FIFO/file) for canis-sense.

use clap::Parser;
use mcu_emu::{McuEmu, McuWorld};
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tracing::info;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value_t = 0.0)]
    force_n: f32,
    #[arg(long, default_value_t = 5000)]
    duration_ms: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("mcu_emu=info")
        .init();
    let args = Args::parse();
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&args.out)
        .await?;
    let mut emu = McuEmu::new();
    if args.force_n > 0.0 {
        emu.set_world(McuWorld::dog_on_mat(args.force_n));
    }
    let ticks = args.duration_ms / 50;
    info!(?args.out, ticks, "streaming MCU frames");
    for _ in 0..ticks {
        let bytes = emu.tick_50ms();
        file.write_all(&bytes).await?;
        file.flush().await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Ok(())
}
