//! Realistic dual-terminal presence simulation.
//!
//! Spawns two emulated dogs with MCU world physics → edge fusion → cloud presence.

use canis_edge::{EdgeAgent, EdgeConfig};
use chrono::Utc;
use clap::Parser;
use mcu_emu::{McuEmu, McuWorld};
use protocol::{DogId, TerminalId};
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    api: String,
    #[arg(long, default_value = "canis-dev-secret")]
    secret: String,
    /// Run scripted scenario and exit.
    #[arg(long, default_value_t = true)]
    scenario_presence: bool,
}

#[derive(Debug, Deserialize)]
struct EnrollResponse {
    terminal_id: TerminalId,
    dog_id: DogId,
    token: String,
}

async fn enroll(api: &str) -> anyhow::Result<EnrollResponse> {
    let client = reqwest::Client::new();
    let res = client
        .post(format!("{}/v1/dev/enroll", api.trim_end_matches('/')))
        .json(&serde_json::json!({}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(res)
}

struct VirtualTerminal {
    name: String,
    mcu: McuEmu,
    edge: EdgeAgent,
}

impl VirtualTerminal {
    fn new(name: &str, enroll: EnrollResponse, api: &str) -> Self {
        let edge = EdgeAgent::new(EdgeConfig {
            api_base: api.to_string(),
            terminal_id: enroll.terminal_id,
            dog_id: enroll.dog_id,
            token: enroll.token,
            publish_ms: 2000,
        });
        Self {
            name: name.into(),
            mcu: McuEmu::new(),
            edge,
        }
    }

    async fn tick(&mut self, dt_ms: u64) -> anyhow::Result<()> {
        let bytes = self.mcu.tick_50ms();
        // mcu ticks are 50ms; may call multiple
        let snaps = self.edge.ingest_uart(&bytes, dt_ms);
        for s in snaps {
            if s.flipped {
                info!(terminal = %self.name, present = s.present, "presence flip");
                self.edge.publish_now().await?;
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("sim_dog=info,canis_edge=info")
        .init();
    let args = Args::parse();

    // wait for API
    let client = reqwest::Client::new();
    for _ in 0..50 {
        if client
            .get(format!("{}/healthz", args.api))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let a = enroll(&args.api).await?;
    let b = enroll(&args.api).await?;
    info!(dog_a = %a.dog_id, dog_b = %b.dog_id, "enrolled terminals");

    let mut term_a = VirtualTerminal::new("A", a, &args.api);
    let mut term_b = VirtualTerminal::new("B", b, &args.api);

    // Phase 1: empty mats
    for _ in 0..20 {
        term_a.tick(50).await?;
        term_b.tick(50).await?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    // Phase 2: both dogs step on
    info!("dogs step onto mats");
    term_a.mcu.set_world(McuWorld::dog_on_mat(140.0));
    term_b.mcu.set_world(McuWorld::dog_on_mat(95.0));

    for i in 0..40 {
        term_a.tick(50).await?;
        term_b.tick(50).await?;
        if i % 10 == 0 {
            term_a.edge.publish_now().await?;
            term_b.edge.publish_now().await?;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let list: Vec<serde_json::Value> = client
        .get(format!("{}/v1/presence", args.api))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    info!(count = list.len(), "online dogs");
    if list.len() < 2 {
        anyhow::bail!("expected 2 present dogs, got {}", list.len());
    }

    // Phase 3: dog A leaves
    info!("dog A leaves mat");
    term_a.mcu.set_world(McuWorld::empty());
    for i in 0..60 {
        term_a.tick(50).await?;
        term_b.tick(50).await?;
        if i % 10 == 0 {
            term_a.edge.publish_now().await?;
            term_b.edge.publish_now().await?;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let list: Vec<serde_json::Value> = client
        .get(format!("{}/v1/presence", args.api))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    info!(count = list.len(), "online after A left");
    if list.len() != 1 {
        anyhow::bail!("expected 1 present dog after A left, got {}", list.len());
    }

    info!(ts = %Utc::now(), "presence scenario PASS");
    let _ = Uuid::new_v4(); // keep uuid used if needed
    Ok(())
}
