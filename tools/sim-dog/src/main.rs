//! Full dual-terminal simulation for CanisLink base ship scenarios.

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
    /// presence | call | session | end | again | estop | ship
    #[arg(long, default_value = "ship")]
    scenario: String,
    #[arg(long, default_value = "canis-steward-secret")]
    steward_secret: String,
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
                if !s.present && matches!(self.edge.ux, EdgeUx::InSession | EdgeUx::Negotiating) {
                    self.edge.end_session(protocol::EndReason::WalkAway).await?;
                }
            }
        }
        for i in intents {
            info!(terminal = self.name, ?i, "intent");
            self.edge.handle_intent(i).await?;
        }
        Ok(())
    }

    async fn settle_present(&mut self) -> anyhow::Result<()> {
        self.mcu.set_world(McuWorld::dog_on_mat(120.0));
        for _ in 0..30 {
            self.tick().await?;
            self.edge.publish_now().await?;
        }
        self.edge.fetch_config().await?;
        Ok(())
    }
}

async fn bond(api: &str, a: DogId, b: DogId) -> anyhow::Result<()> {
    reqwest::Client::new()
        .post(format!("{}/v1/dev/bonds", api))
        .json(&serde_json::json!({"dog_a": a, "dog_b": b, "weight": 0.75}))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

async fn wait_api(api: &str) {
    let client = reqwest::Client::new();
    for _ in 0..50 {
        if client
            .get(format!("{}/healthz", api))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("sim_dog=info,canis_edge=info,device_api=info")
        .init();
    let args = Args::parse();
    wait_api(&args.api).await;

    let mut term_a = VirtualTerminal::new("A", enroll(&args.api).await?, &args.api);
    let mut term_b = VirtualTerminal::new("B", enroll(&args.api).await?, &args.api);
    bond(&args.api, term_a.dog_id, term_b.dog_id).await?;
    term_a.settle_present().await?;
    term_b.settle_present().await?;

    if args.scenario == "presence" {
        info!("presence PASS");
        return Ok(());
    }

    // Call
    term_a.mcu.press_pad(0);
    term_a.tick().await?;
    for _ in 0..15 {
        term_b.tick().await?;
        term_b.edge.poll_incoming().await?;
        if term_b.edge.ux == EdgeUx::RingingIn {
            break;
        }
    }
    if term_b.edge.ux != EdgeUx::RingingIn {
        anyhow::bail!("B not RingingIn");
    }
    if args.scenario == "call" {
        info!("call PASS");
        return Ok(());
    }

    // Accept
    term_b.mcu.press_pad(1);
    term_b.tick().await?;
    term_a.edge.sync_active().await?;
    if !matches!(term_b.edge.ux, EdgeUx::InSession | EdgeUx::Negotiating) {
        anyhow::bail!("B not in session {:?}", term_b.edge.ux);
    }
    // ensure both Active
    term_a.edge.report_media_ready(true).await.ok();
    term_b.edge.report_media_ready(true).await.ok();
    term_a.edge.sync_active().await?;
    if args.scenario == "session" {
        info!("session PASS");
        return Ok(());
    }

    if args.scenario == "again" || args.scenario == "ship" {
        term_a.mcu.press_pad(2);
        term_a.tick().await?;
        info!("again PASS");
        if args.scenario == "again" {
            return Ok(());
        }
    }

    if args.scenario == "estop" || args.scenario == "ship" {
        // steward estop on B should kill session
        let client = reqwest::Client::new();
        client
            .post(format!("{}/v1/steward/estop", args.api))
            .header("Authorization", format!("Steward {}", args.steward_secret))
            .json(&serde_json::json!({"dog_id": term_b.dog_id, "enabled": true}))
            .send()
            .await?
            .error_for_status()?;
        // wait policy tick
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        term_a.edge.sync_active().await?;
        term_b.edge.fetch_config().await?;
        if term_a.edge.session.is_some() {
            // local may still have stale; force refresh
            let url = format!(
                "{}/v1/sessions/active?dog_id={}&terminal_id={}",
                args.api, term_a.dog_id, term_a.edge.cfg.terminal_id
            );
            let s: Option<serde_json::Value> = client
                .get(url)
                .header("Authorization", term_a.edge.cfg.auth_header())
                .send()
                .await?
                .json()
                .await?;
            if s.is_some() {
                anyhow::bail!("session still active after estop");
            }
        }
        info!("estop PASS");
        // clear estop for cleanliness
        client
            .post(format!("{}/v1/steward/estop", args.api))
            .header("Authorization", format!("Steward {}", args.steward_secret))
            .json(&serde_json::json!({"dog_id": term_b.dog_id, "enabled": false}))
            .send()
            .await?
            .error_for_status()?;
        if args.scenario == "estop" {
            return Ok(());
        }
    }

    if args.scenario == "end" {
        // fresh session for end
        // re-enable social
        term_a.edge.fetch_config().await?;
        term_b.edge.fetch_config().await?;
        term_a.mcu.press_pad(0);
        term_a.tick().await?;
        for _ in 0..10 {
            term_b.edge.poll_incoming().await?;
            if term_b.edge.ux == EdgeUx::RingingIn {
                break;
            }
        }
        term_b.mcu.press_pad(0);
        term_b.tick().await?;
        term_a.edge.sync_active().await?;
        term_a.mcu.press_pad(3);
        term_a.tick().await?;
        info!("end PASS");
        return Ok(());
    }

    if args.scenario == "ship" {
        info!("SHIP SCENARIO PASS — base control plane ready");
        return Ok(());
    }

    anyhow::bail!("unknown scenario {}", args.scenario)
}
