//! Dual-terminal presence + Call + Accept simulation.

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
    #[arg(long, default_value = "session")]
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
                if !s.present && self.edge.ux == EdgeUx::InSession {
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("sim_dog=info,canis_edge=info,device_api=info")
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

    term_a.mcu.set_world(McuWorld::dog_on_mat(140.0));
    term_b.mcu.set_world(McuWorld::dog_on_mat(95.0));
    for _ in 0..30 {
        term_a.tick().await?;
        term_b.tick().await?;
        term_a.edge.publish_now().await?;
        term_b.edge.publish_now().await?;
    }

    info!("A Call");
    term_a.mcu.press_pad(0);
    term_a.tick().await?;

    for _ in 0..10 {
        term_b.tick().await?;
        term_b.edge.poll_incoming().await?;
        if term_b.edge.ux == EdgeUx::RingingIn {
            break;
        }
    }
    if term_b.edge.ux != EdgeUx::RingingIn {
        anyhow::bail!("B not ringing");
    }

    if args.scenario == "call" {
        info!("call scenario PASS");
        return Ok(());
    }

    info!("B engages pad (accept)");
    term_b.mcu.press_pad(1); // Play pad = engage
    term_b.tick().await?;
    if term_b.edge.ux != EdgeUx::InSession {
        anyhow::bail!("B not in session: {:?}", term_b.edge.ux);
    }
    // A should learn session via... for now A still RingingOut until we poll/active
    // Notify A by checking active session endpoint is optional; set A session from accept side only.
    // For realism, A polls active — skip: mark A when B accepts by A re-fetch...
    // Simpler: after B accepts, A still RingingOut in edge state; production would WS push.
    // For sim: A polls sessions/active
    let url = format!(
        "{}/v1/sessions/active?dog_id={}&terminal_id={}",
        args.api, term_a.dog_id, term_a.edge.cfg.terminal_id
    );
    let sess: Option<serde_json::Value> = client
        .get(&url)
        .header("Authorization", term_a.edge.cfg.auth_header())
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if sess.is_none() {
        anyhow::bail!("A has no active session after B accept");
    }
    term_a.edge.session = Some(serde_json::from_value(sess.clone().unwrap())?);
    term_a.edge.ux = EdgeUx::InSession;

    info!(session = %sess.unwrap()["id"], "session active both sides");

    if args.scenario == "session" {
        info!("session accept scenario PASS");
        return Ok(());
    }

    // walk-away / done
    info!("A presses Done");
    term_a.mcu.press_pad(3);
    term_a.tick().await?;
    if term_a.edge.ux == EdgeUx::InSession {
        anyhow::bail!("A still in session after Done");
    }
    info!("end scenario PASS");
    Ok(())
}
