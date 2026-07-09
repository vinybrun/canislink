//! Edge agent — presence + call invite.

use canis_sense::{SensePipeline, SenseSnapshot};
use chrono::Utc;
use protocol::mcu::{ButtonPayload, FrameDecoder, MsgType};
use protocol::{
    CreateInviteRequest, CreateInviteResponse, DogId, IncomingInviteOffer, Intent, InviteMode,
    PresenceReport, TerminalId, PRESENCE_PUBLISH_MS,
};
use reqwest::Client;
use std::time::Duration;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct EdgeConfig {
    pub api_base: String,
    pub terminal_id: TerminalId,
    pub dog_id: DogId,
    pub token: String,
    pub publish_ms: u64,
}

impl EdgeConfig {
    pub fn auth_header(&self) -> String {
        format!("Device {}:{}", self.terminal_id, self.token)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeUx {
    IdleEmpty,
    IdlePresent,
    Inviting,
    RingingOut,
    RingingIn,
}

#[derive(Debug)]
pub struct EdgeAgent {
    pub cfg: EdgeConfig,
    pub sense: SensePipeline,
    client: Client,
    seq: u64,
    last_published_present: Option<bool>,
    pub ux: EdgeUx,
    pub last_offer: Option<IncomingInviteOffer>,
    decoder: FrameDecoder,
}

impl EdgeAgent {
    pub fn new(cfg: EdgeConfig) -> Self {
        Self {
            cfg,
            sense: SensePipeline::new(),
            client: Client::new(),
            seq: 0,
            last_published_present: None,
            ux: EdgeUx::IdleEmpty,
            last_offer: None,
            decoder: FrameDecoder::new(),
        }
    }

    pub fn social_armed(&self) -> bool {
        self.sense.filter().present() && matches!(self.ux, EdgeUx::IdlePresent | EdgeUx::IdleEmpty)
    }

    fn refresh_ux_from_presence(&mut self) {
        let present = self.sense.filter().present();
        match (present, self.ux) {
            (true, EdgeUx::IdleEmpty) => self.ux = EdgeUx::IdlePresent,
            (false, EdgeUx::IdlePresent) => self.ux = EdgeUx::IdleEmpty,
            (false, EdgeUx::RingingIn) => {
                // walk away from lure = ignore
                self.ux = EdgeUx::IdleEmpty;
                self.last_offer = None;
            }
            _ => {}
        }
    }

    pub fn ingest_sample(
        &mut self,
        force_n: f32,
        tof_mm: Option<u16>,
        motion: bool,
        dt_ms: u64,
    ) -> SenseSnapshot {
        let snap = self.sense.push_sample(force_n, tof_mm, motion, dt_ms);
        self.refresh_ux_from_presence();
        snap
    }

    pub fn ingest_uart(&mut self, bytes: &[u8], dt_ms: u64) -> (Vec<SenseSnapshot>, Vec<Intent>) {
        let snaps = self.sense.push_bytes(bytes, dt_ms);
        self.refresh_ux_from_presence();
        let frames = self.decoder.push(bytes);
        let mut intents = Vec::new();
        for fr in frames {
            if fr.msg == MsgType::Button {
                if let Some(b) = ButtonPayload::from_bytes(&fr.payload) {
                    if b.event == 1 {
                        if let Some(i) = pad_to_intent(b.pad) {
                            intents.push(i);
                        }
                    }
                }
            }
        }
        // Also sense frames already handled by push_bytes which uses its own decoder —
        // wait, SensePipeline has its own decoder, and we push same bytes to both. Good.
        (snaps, intents)
    }

    fn next_report(&mut self) -> PresenceReport {
        self.seq += 1;
        let f = self.sense.filter();
        PresenceReport {
            dog_id: self.cfg.dog_id,
            terminal_id: self.cfg.terminal_id,
            present: f.present(),
            confidence: f.confidence(),
            force_band: f.force_band(),
            force_n: f.last_force_n(),
            tof_mm: f.last_tof_mm(),
            ts: Utc::now(),
            seq: self.seq,
        }
    }

    pub async fn publish_now(&mut self) -> anyhow::Result<()> {
        let report = self.next_report();
        let url = format!("{}/v1/presence", self.cfg.api_base.trim_end_matches('/'));
        let res = self
            .client
            .post(&url)
            .header("Authorization", self.cfg.auth_header())
            .json(&report)
            .send()
            .await?;
        if !res.status().is_success() {
            anyhow::bail!("presence publish failed: {}", res.status());
        }
        self.last_published_present = Some(report.present);
        Ok(())
    }

    pub async fn call(&mut self, to: Option<DogId>) -> anyhow::Result<CreateInviteResponse> {
        if !self.sense.filter().present() {
            anyhow::bail!("not present — Call ignored");
        }
        self.ux = EdgeUx::Inviting;
        let url = format!("{}/v1/invites", self.cfg.api_base.trim_end_matches('/'));
        let body = serde_json::json!({
            "mode": InviteMode::Portal,
            "to_dog": to,
            "dog_id": self.cfg.dog_id,
            "terminal_id": self.cfg.terminal_id,
        });
        let res = self
            .client
            .post(&url)
            .header("Authorization", self.cfg.auth_header())
            .json(&body)
            .send()
            .await?;
        if !res.status().is_success() {
            self.ux = EdgeUx::IdlePresent;
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            anyhow::bail!("invite failed: {status} {text}");
        }
        let resp: CreateInviteResponse = res.json().await?;
        self.ux = EdgeUx::RingingOut;
        info!(invite = %resp.invite.id, to = %resp.invite.to_dog, "ringing out");
        Ok(resp)
    }

    pub async fn poll_incoming(&mut self) -> anyhow::Result<Option<IncomingInviteOffer>> {
        let url = format!(
            "{}/v1/invites/incoming?dog_id={}&terminal_id={}",
            self.cfg.api_base.trim_end_matches('/'),
            self.cfg.dog_id,
            self.cfg.terminal_id
        );
        let res = self
            .client
            .get(&url)
            .header("Authorization", self.cfg.auth_header())
            .send()
            .await?;
        if !res.status().is_success() {
            warn!(status = %res.status(), "incoming poll failed");
            return Ok(None);
        }
        let offer: Option<IncomingInviteOffer> = res.json().await?;
        if let Some(ref o) = offer {
            if self.ux != EdgeUx::RingingOut {
                self.ux = EdgeUx::RingingIn;
                self.last_offer = Some(o.clone());
                info!(
                    invite = %o.invite.id,
                    from = %o.invite.from_dog,
                    pattern = %o.lure.led_pattern,
                    "lure active (dog-native, no human push)"
                );
            }
        }
        Ok(offer)
    }

    pub fn publish_interval() -> Duration {
        Duration::from_millis(PRESENCE_PUBLISH_MS)
    }
}

fn pad_to_intent(pad: u8) -> Option<Intent> {
    match pad {
        0 => Some(Intent::Call),
        1 => Some(Intent::Play),
        2 => Some(Intent::Again),
        3 => Some(Intent::Done),
        _ => None,
    }
}

// silence
#[allow(dead_code)]
fn _req(_: CreateInviteRequest) {}
#[allow(dead_code)]
fn _dbg() {
    debug!("x");
}
