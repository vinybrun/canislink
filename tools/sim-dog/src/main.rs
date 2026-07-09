//! Dual-terminal presence + Call invite simulation.

use canis_edge::{EdgeAgent, EdgeConfig, EdgeUx};
use clap::Parser;
use mcu_emu::{McuEmu, McuWorld};
use protocol::{DogId, TerminalId};
use serde::Deserialize;
use tracing::info;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    api: String,
    #[arg(long, default_value = "call")]
    scenario: String,
}

#[derive(Debug, Deserialize)]
struct EnrollResponse {
    terminal_id: TerminalId,
    dog_id: DogId,
    token: String,
}

async fn enroll(api: &str) -> anyhow::Result<EnrollResponse> {
    Ok(reqwest::Client::new()
        .post(format!("{}/v1/dev/enroll", api.trim_end_matches('/')))
        .json(&serde_json::json!({}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

struct VirtualTerminal {
    name: &'static str,
    mcu: McuEmu,
    edge: EdgeAgent,
    dog_id: DogId,
}

impl VirtualTerminal {
    fn new(name: &'static str, enroll: EnrollResponse, api: &str) -> Self {
        let dog_id = enroll.dog_id;
        Self {
            name,
            mcu: McuEmu::new(),
            edge: EdgeAgent::new(EdgeConfig {
                api_base: api.to_string(),
                terminal_id: enroll.terminal_id,
                dog_id: enroll.dog_id,
                token: enroll.token,
                publish_ms: 2000,
            }),
            dog_id,
        }
    }

    async fn tick(&mut self) -> anyhow::Result<()> {
        let bytes = self.mcu.tick_50ms();
        let (snaps, intents) = self.edge.ingest_uart(&bytes, 50);
        for s in snaps {
            if s.flipped {
                info!(terminal = self.name, present = s.present, "presence flip");
                self.edge.publish_now().await?;
            }
        }
        for i in intents {
            info!(terminal = self.name, ?i, "intent from pad");
            if matches!(i, protocol::Intent::Call) {
                match self.edge.call(None).await {
                    Ok(r) => info!(terminal = self.name, invite = %r.invite.id, "call placed"),
                    Err(e) => info!(terminal = self.name, error = %e, "call failed"),
                }
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

    let mut term_a = VirtualTerminal::new("A", enroll(&args.api).await?, &args.api);
    let mut term_b = VirtualTerminal::new("B", enroll(&args.api).await?, &args.api);

    // mutual bond
    client
        .post(format!("{}/v1/dev/bonds", args.api))
        .json(&serde_json::json!({
            "dog_a": term_a.dog_id,
            "dog_b": term_b.dog_id,
            "weight": 0.7
        }))
        .send()
        .await?
        .error_for_status()?;
    info!("bonded A ↔ B");

    // both on mats
    term_a.mcu.set_world(McuWorld::dog_on_mat(140.0));
    term_b.mcu.set_world(McuWorld::dog_on_mat(95.0));
    for _ in 0..30 {
        term_a.tick().await?;
        term_b.tick().await?;
        term_a.edge.publish_now().await?;
        term_b.edge.publish_now().await?;
    }

    let list: Vec<serde_json::Value> = client
        .get(format!("{}/v1/presence", args.api))
        .send()
        .await?
        .json()
        .await?;
    if list.len() < 2 {
        anyhow::bail!("need 2 present, got {}", list.len());
    }

    if args.scenario == "presence" {
        info!("presence-only scenario PASS");
        return Ok(());
    }

    // Dog A presses Call pad
    info!("dog A presses Call");
    term_a.mcu.press_pad(0);
    term_a.tick().await?;

    // B polls for lure
    let mut saw_lure = false;
    for _ in 0..20 {
        term_b.tick().await?;
        if let Some(offer) = term_b.edge.poll_incoming().await? {
            info!(
                invite = %offer.invite.id,
                from = %offer.invite.from_dog,
                "B received lure"
            );
            saw_lure = true;
            assert_eq!(term_b.edge.ux, EdgeUx::RingingIn);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    if !saw_lure {
        anyhow::bail!("B never received incoming invite lure");
    }
    assert_eq!(term_a.edge.ux, EdgeUx::RingingOut);

    info!("call invite scenario PASS (no human in path)");
    Ok(())
}
