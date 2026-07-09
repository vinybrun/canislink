//! Standalone sense reader — reads UART frames from a file/FIFO (emu path).

use canis_sense::SensePipeline;
use clap::Parser;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use tracing::info;

#[derive(Parser, Debug)]
struct Args {
    /// Path to UART byte stream (FIFO or file written by mcu-emu).
    #[arg(long)]
    uart: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("canis_sense=info")
        .init();
    let args = Args::parse();
    let mut file = tokio::fs::File::open(&args.uart).await?;
    let mut pipe = SensePipeline::new();
    let mut buf = [0u8; 256];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            continue;
        }
        for snap in pipe.push_bytes(&buf[..n], 50) {
            if snap.flipped {
                info!(
                    present = snap.present,
                    force = snap.force_n,
                    conf = snap.confidence,
                    "presence flipped"
                );
            }
        }
    }
}
