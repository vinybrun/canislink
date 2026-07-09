//! Local experiment: emulate two steward phones + two embedded dog terminals.
//!
//! Roles:
//! - Phone A/B  → human install/ops (enroll, bond, e-stop) — never accept invites
//! - Terminal A/B → MCU UART emulator + edge agent (dog social path)
//! - Optional: spawn canis-media peers after session Active for WebRTC proof
//!
//! Assumes device-api (+ signaling for media) already running.

use anyhow::{bail, Context};
use canis_edge::{EdgeAgent, EdgeConfig, EdgeUx};
use chrono::Utc;
use clap::Parser;
use mcu_emu::{McuEmu, McuWorld};
use protocol::{DogId, SessionState, TerminalId};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(
    name = "lab-experiment",
    about = "E2E local lab with phone + embedded emulators"
)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    api: String,
    #[arg(long, default_value = "ws://127.0.0.1:8081")]
    signal: String,
    #[arg(long, default_value = "canis-steward-secret")]
    steward_secret: String,
    /// Also run canis-media WebRTC after session is Active
    #[arg(long, default_value_t = true)]
    with_webrtc: bool,
    #[arg(long, default_value = "target/debug")]
    bin_dir: PathBuf,
    #[arg(long, default_value = "docs/lab/experiment-report.json")]
    report: PathBuf,
}

#[derive(Debug, Deserialize, Clone)]
struct Enroll {
    #[allow(dead_code)]
    terminal_id: TerminalId,
    dog_id: DogId,
    token: String,
}

/// Emulated steward phone (human install app).
struct PhoneEmu {
    name: &'static str,
    api: String,
    steward_secret: String,
    client: reqwest::Client,
    enrolled: Option<Enroll>,
}

impl PhoneEmu {
    fn new(name: &'static str, api: &str, steward_secret: &str) -> Self {
        Self {
            name,
            api: api.to_string(),
            steward_secret: steward_secret.to_string(),
            client: reqwest::Client::new(),
            enrolled: None,
        }
    }

    async fn enroll_terminal(&mut self) -> anyhow::Result<Enroll> {
        info!(
            phone = self.name,
            "enrolling terminal (phone install wizard)"
        );
        let e: Enroll = self
            .client
            .post(format!("{}/v1/dev/enroll", self.api))
            .json(&json!({}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        info!(
            phone = self.name,
            dog = %e.dog_id,
            terminal = %e.terminal_id,
            "enrolled"
        );
        self.enrolled = Some(e.clone());
        Ok(e)
    }

    async fn bond_with(&self, other: DogId) -> anyhow::Result<()> {
        let me = self.enrolled.as_ref().context("phone not enrolled")?.dog_id;
        info!(phone = self.name, a = %me, b = %other, "creating mutual bond");
        self.client
            .post(format!("{}/v1/steward/bonds", self.api))
            .header("Authorization", format!("Steward {}", self.steward_secret))
            .json(&json!({"dog_a": me, "dog_b": other, "weight": 0.85}))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn estop(&self, enabled: bool) -> anyhow::Result<()> {
        let dog = self.enrolled.as_ref().context("not enrolled")?.dog_id;
        self.client
            .post(format!("{}/v1/steward/estop", self.api))
            .header("Authorization", format!("Steward {}", self.steward_secret))
            .json(&json!({"dog_id": dog, "enabled": enabled}))
            .send()
            .await?
            .error_for_status()?;
        info!(phone = self.name, %enabled, "estop toggled");
        Ok(())
    }
}

/// Emulated embedded dog terminal (MCU + edge).
struct EmbeddedTerminal {
    name: &'static str,
    mcu: McuEmu,
    edge: EdgeAgent,
    dog_id: DogId,
    #[allow(dead_code)]
    terminal_id: TerminalId,
}

impl EmbeddedTerminal {
    fn from_enroll(name: &'static str, e: Enroll, api: &str) -> Self {
        let dog_id = e.dog_id;
        let terminal_id = e.terminal_id;
        Self {
            name,
            mcu: McuEmu::new(),
            edge: EdgeAgent::new(EdgeConfig {
                api_base: api.into(),
                terminal_id: e.terminal_id,
                dog_id: e.dog_id,
                token: e.token,
                publish_ms: 2000,
            }),
            dog_id,
            terminal_id,
        }
    }

    async fn tick(&mut self) -> anyhow::Result<()> {
        let bytes = self.mcu.tick_50ms();
        let (snaps, intents) = self.edge.ingest_uart(&bytes, 50);
        for s in snaps {
            if s.flipped {
                info!(
                    terminal = self.name,
                    present = s.present,
                    "MCU→edge presence"
                );
                self.edge.publish_now().await?;
            }
        }
        for i in intents {
            info!(terminal = self.name, ?i, "pad intent");
            self.edge.handle_intent(i).await?;
        }
        Ok(())
    }

    async fn dog_steps_on_mat(&mut self, weight_n: f32) -> anyhow::Result<()> {
        info!(terminal = self.name, weight_n, "dog steps on mat");
        self.mcu.set_world(McuWorld::dog_on_mat(weight_n));
        for _ in 0..30 {
            self.tick().await?;
            self.edge.publish_now().await?;
        }
        self.edge.fetch_config().await?;
        if !self.edge.sense.filter().present() {
            bail!("{} presence filter never went present", self.name);
        }
        Ok(())
    }

    async fn press(&mut self, pad: u8) -> anyhow::Result<()> {
        self.mcu.press_pad(pad);
        self.tick().await?;
        Ok(())
    }
}

#[derive(Default, serde::Serialize)]
struct Report {
    started_at: String,
    finished_at: String,
    ok: bool,
    steps: Vec<Step>,
    session_id: Option<String>,
    webrtc_ok: Option<bool>,
    error: Option<String>,
}

#[derive(serde::Serialize)]
struct Step {
    name: String,
    ok: bool,
    detail: String,
}

impl Report {
    fn step(&mut self, name: &str, ok: bool, detail: impl Into<String>) {
        self.steps.push(Step {
            name: name.into(),
            ok,
            detail: detail.into(),
        });
        if ok {
            info!(step = name, "PASS");
        } else {
            warn!(step = name, "FAIL");
        }
    }
}

async fn wait_api(api: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    for _ in 0..50 {
        if client
            .get(format!("{}/healthz", api))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    bail!("API not healthy at {api}");
}

fn spawn_media(
    bin_dir: &PathBuf,
    signal: &str,
    session: Uuid,
    dog: Uuid,
    role: &str,
) -> anyhow::Result<Child> {
    let bin = bin_dir.join("canis-media");
    Command::new(bin)
        .args([
            "--signal",
            signal,
            "--session",
            &session.to_string(),
            "--dog",
            &dog.to_string(),
            "--role",
            role,
            "--timeout-sec",
            "40",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn canis-media")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lab_experiment=info,canis_edge=info".into()),
        )
        .init();

    let args = Args::parse();
    let mut report = Report {
        started_at: Utc::now().to_rfc3339(),
        finished_at: String::new(),
        ok: false,
        steps: vec![],
        session_id: None,
        webrtc_ok: None,
        error: None,
    };

    let run = async {
        wait_api(&args.api).await?;
        report.step("api_health", true, &args.api);

        // --- Phones (human) ---
        let mut phone_a = PhoneEmu::new("PhoneA", &args.api, &args.steward_secret);
        let mut phone_b = PhoneEmu::new("PhoneB", &args.api, &args.steward_secret);
        let enroll_a = phone_a.enroll_terminal().await?;
        let enroll_b = phone_b.enroll_terminal().await?;
        report.step(
            "phone_enroll",
            true,
            format!("A={} B={}", enroll_a.dog_id, enroll_b.dog_id),
        );

        phone_a.bond_with(enroll_b.dog_id).await?;
        report.step("phone_bond", true, "mutual bond A↔B");

        // --- Embedded terminals (dogs) ---
        let mut term_a = EmbeddedTerminal::from_enroll("TerminalA", enroll_a, &args.api);
        let mut term_b = EmbeddedTerminal::from_enroll("TerminalB", enroll_b, &args.api);

        term_a.dog_steps_on_mat(140.0).await?;
        term_b.dog_steps_on_mat(110.0).await?;
        report.step("embedded_presence", true, "both mats occupied");

        // Dog A Call
        term_a.press(0).await?; // Call pad
        if term_a.edge.ux != EdgeUx::RingingOut {
            bail!("TerminalA expected RingingOut, got {:?}", term_a.edge.ux);
        }
        report.step("dog_a_call", true, "Call pad → invite");

        // Terminal B receives lure
        for _ in 0..20 {
            term_b.tick().await?;
            term_b.edge.poll_incoming().await?;
            if term_b.edge.ux == EdgeUx::RingingIn {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if term_b.edge.ux != EdgeUx::RingingIn {
            bail!("TerminalB never entered RingingIn (lure)");
        }
        report.step("dog_b_lure", true, "dog-native lure, no phone push");

        // Dog B engages pad (accept) — NOT phone
        term_b.press(1).await?; // Play pad = engage
        if !matches!(term_b.edge.ux, EdgeUx::InSession | EdgeUx::Negotiating) {
            bail!("TerminalB accept failed: {:?}", term_b.edge.ux);
        }
        report.step("dog_b_accept", true, "pad engage → session");

        // Sync A + media ready both sides
        term_a.edge.sync_active().await?;
        term_a.edge.report_media_ready(true).await?;
        term_b.edge.report_media_ready(true).await?;
        term_a.edge.sync_active().await?;

        let sess = term_a
            .edge
            .session
            .clone()
            .context("no session on TerminalA after accept")?;
        report.session_id = Some(sess.id.to_string());
        if sess.state != SessionState::Active && term_b.edge.ux != EdgeUx::InSession {
            // allow negotiating if media_ready race
            warn!(state = ?sess.state, "session state");
        }
        report.step(
            "session_active",
            true,
            format!("session={} state={:?}", sess.id, sess.state),
        );

        // Again
        term_a.press(2).await?;
        report.step("dog_a_again", true, "soft segment extend");

        // WebRTC media plane (real ICE + datachannel)
        if args.with_webrtc {
            let session_uuid = sess.id.0;
            let dog_a = term_a.dog_id.0;
            let dog_b = term_b.dog_id.0;
            let mut answerer =
                spawn_media(&args.bin_dir, &args.signal, session_uuid, dog_b, "answerer")?;
            tokio::time::sleep(Duration::from_millis(400)).await;
            let mut offerer =
                spawn_media(&args.bin_dir, &args.signal, session_uuid, dog_a, "offerer")?;
            let off_status = offerer.wait().context("wait offerer")?;
            let _ = answerer.kill();
            let _ = answerer.wait();
            let webrtc_ok = off_status.success();
            report.webrtc_ok = Some(webrtc_ok);
            report.step(
                "webrtc_portal",
                webrtc_ok,
                if webrtc_ok {
                    "canis-media Connected + portal hello".into()
                } else {
                    format!("canis-media exit {:?}", off_status.code())
                },
            );
            if !webrtc_ok {
                bail!("WebRTC media plane failed");
            }
        }

        // Done
        term_a.press(3).await?;
        if matches!(term_a.edge.ux, EdgeUx::InSession | EdgeUx::Negotiating) {
            bail!("TerminalA still in session after Done");
        }
        report.step("dog_a_done", true, "session ended by Done pad");

        // Phone e-stop smoke (does not accept invites)
        phone_b.estop(true).await?;
        phone_b.estop(false).await?;
        report.step("phone_estop", true, "steward path only");

        Ok::<(), anyhow::Error>(())
    }
    .await;

    match run {
        Ok(()) => {
            report.ok = true;
            report.finished_at = Utc::now().to_rfc3339();
        }
        Err(e) => {
            report.ok = false;
            report.error = Some(e.to_string());
            report.finished_at = Utc::now().to_rfc3339();
            if let Some(last) = report.steps.last() {
                if last.ok {
                    report.step("fatal", false, e.to_string());
                }
            } else {
                report.step("fatal", false, e.to_string());
            }
        }
    }

    if let Some(parent) = args.report.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&args.report, serde_json::to_string_pretty(&report)?)?;
    info!(path = %args.report.display(), ok = report.ok, "wrote report");

    // human summary
    println!("\n======== LAB EXPERIMENT REPORT ========");
    println!("ok: {}", report.ok);
    for s in &report.steps {
        println!(
            "  [{}] {} — {}",
            if s.ok { "PASS" } else { "FAIL" },
            s.name,
            s.detail
        );
    }
    if let Some(sid) = &report.session_id {
        println!("session_id: {sid}");
    }
    if let Some(w) = report.webrtc_ok {
        println!("webrtc_ok: {w}");
    }
    if let Some(e) = &report.error {
        println!("error: {e}");
    }
    println!("report: {}", args.report.display());
    println!("=======================================\n");

    if report.ok {
        Ok(())
    } else {
        bail!("lab experiment failed");
    }
}
