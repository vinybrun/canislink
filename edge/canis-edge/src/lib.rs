//! Edge agent — full base control plane client.

use canis_sense::{SensePipeline, SenseSnapshot};
use chrono::Utc;
use protocol::mcu::{ButtonPayload, FrameDecoder, MsgType};
use protocol::{
    AcceptInviteResponse, ConfigV1, CreateInviteResponse, DogId, EndReason, IncomingInviteOffer,
    Intent, InviteMode, PresenceReport, SessionRecord, SessionState, TerminalId,
    PRESENCE_PUBLISH_MS,
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
    Negotiating,
    InSession,
    Halted,
}

#[derive(Debug)]
pub struct EdgeAgent {
    pub cfg: EdgeConfig,
    pub sense: SensePipeline,
    client: Client,
    seq: u64,
    pub ux: EdgeUx,
    pub last_offer: Option<IncomingInviteOffer>,
    pub session: Option<SessionRecord>,
    pub remote_config: Option<ConfigV1>,
    decoder: FrameDecoder,
    pub social_armed: bool,
}

impl EdgeAgent {
    pub fn new(cfg: EdgeConfig) -> Self {
        Self {
            cfg,
            sense: SensePipeline::new(),
            client: Client::new(),
            seq: 0,
            ux: EdgeUx::IdleEmpty,
            last_offer: None,
            session: None,
            remote_config: None,
            decoder: FrameDecoder::new(),
            social_armed: true,
        }
    }

    fn refresh_ux_from_presence(&mut self) {
        let present = self.sense.filter().present();
        match (present, self.ux) {
            (true, EdgeUx::IdleEmpty) => self.ux = EdgeUx::IdlePresent,
            (false, EdgeUx::IdlePresent) => self.ux = EdgeUx::IdleEmpty,
            (false, EdgeUx::RingingIn) => {
                self.ux = EdgeUx::IdleEmpty;
                self.last_offer = None;
            }
            _ => {}
        }
        self.recompute_armed();
    }

    fn recompute_armed(&mut self) {
        let present = self.sense.filter().present();
        let estop = self
            .remote_config
            .as_ref()
            .map(|c| c.emergency_stop)
            .unwrap_or(false);
        let disabled = self
            .remote_config
            .as_ref()
            .map(|c| c.social_disabled)
            .unwrap_or(false);
        self.social_armed = present && !estop && !disabled && self.ux != EdgeUx::Halted;
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
        Ok(())
    }

    pub async fn fetch_config(&mut self) -> anyhow::Result<ConfigV1> {
        let url = format!(
            "{}/v1/config?dog_id={}&terminal_id={}",
            self.cfg.api_base.trim_end_matches('/'),
            self.cfg.dog_id,
            self.cfg.terminal_id
        );
        let res = self
            .client
            .get(&url)
            .header("Authorization", self.cfg.auth_header())
            .send()
            .await?
            .error_for_status()?;
        let cfg: ConfigV1 = res.json().await?;
        if cfg.emergency_stop {
            self.ux = EdgeUx::Halted;
            if self.session.is_some() {
                let _ = self.end_session(EndReason::EmergencyStop).await;
            }
        }
        self.remote_config = Some(cfg.clone());
        self.recompute_armed();
        Ok(cfg)
    }

    pub async fn call(&mut self, to: Option<DogId>) -> anyhow::Result<CreateInviteResponse> {
        if !self.social_armed {
            anyhow::bail!("social not armed");
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
            anyhow::bail!(
                "invite failed: {} {}",
                res.status(),
                res.text().await.unwrap_or_default()
            );
        }
        let resp: CreateInviteResponse = res.json().await?;
        self.ux = EdgeUx::RingingOut;
        info!(invite = %resp.invite.id, "ringing out");
        Ok(resp)
    }

    pub async fn poll_incoming(&mut self) -> anyhow::Result<Option<IncomingInviteOffer>> {
        if !self.social_armed && self.ux != EdgeUx::RingingIn {
            return Ok(None);
        }
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
            return Ok(None);
        }
        let offer: Option<IncomingInviteOffer> = res.json().await?;
        if let Some(ref o) = offer {
            if !matches!(
                self.ux,
                EdgeUx::RingingOut | EdgeUx::InSession | EdgeUx::Negotiating
            ) {
                self.ux = EdgeUx::RingingIn;
                self.last_offer = Some(o.clone());
                info!(invite = %o.invite.id, "lure active");
            }
        }
        Ok(offer)
    }

    pub async fn accept_incoming(&mut self) -> anyhow::Result<AcceptInviteResponse> {
        let offer = self
            .last_offer
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no offer"))?;
        let url = format!(
            "{}/v1/invites/{}/accept",
            self.cfg.api_base.trim_end_matches('/'),
            offer.invite.id
        );
        let body = serde_json::json!({
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
            anyhow::bail!("accept failed: {}", res.status());
        }
        let resp: AcceptInviteResponse = res.json().await?;
        self.session = Some(resp.session.clone());
        self.last_offer = None;
        self.ux = EdgeUx::Negotiating;
        // media stub: immediately declare ready
        self.report_media_ready(true).await?;
        Ok(resp)
    }

    pub async fn report_media_ready(&mut self, ready: bool) -> anyhow::Result<()> {
        let sess = self
            .session
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no session"))?;
        let url = format!(
            "{}/v1/sessions/{}/media_ready",
            self.cfg.api_base.trim_end_matches('/'),
            sess.id
        );
        let body = serde_json::json!({
            "dog_id": self.cfg.dog_id,
            "terminal_id": self.cfg.terminal_id,
            "ready": ready,
        });
        let res = self
            .client
            .post(&url)
            .header("Authorization", self.cfg.auth_header())
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        let body: serde_json::Value = res.json().await?;
        if let Some(session) = body.get("session") {
            let s: SessionRecord = serde_json::from_value(session.clone())?;
            self.session = Some(s.clone());
            if s.state == SessionState::Active || body["both_ready"].as_bool() == Some(true) {
                self.ux = EdgeUx::InSession;
                info!(session = %s.id, "portal Active (media stub path)");
            }
        }
        Ok(())
    }

    pub async fn again(&mut self) -> anyhow::Result<()> {
        let sess = self
            .session
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no session"))?;
        let url = format!(
            "{}/v1/sessions/{}/again",
            self.cfg.api_base.trim_end_matches('/'),
            sess.id
        );
        let body = serde_json::json!({
            "dog_id": self.cfg.dog_id,
            "terminal_id": self.cfg.terminal_id,
        });
        let res = self
            .client
            .post(&url)
            .header("Authorization", self.cfg.auth_header())
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        let body: serde_json::Value = res.json().await?;
        if let Some(session) = body.get("session") {
            self.session = Some(serde_json::from_value(session.clone())?);
            info!("segment extended (Again)");
        }
        Ok(())
    }

    pub async fn end_session(&mut self, reason: EndReason) -> anyhow::Result<()> {
        let Some(sess) = self.session.clone() else {
            return Ok(());
        };
        let url = format!(
            "{}/v1/sessions/{}/end",
            self.cfg.api_base.trim_end_matches('/'),
            sess.id
        );
        let body = serde_json::json!({
            "dog_id": self.cfg.dog_id,
            "terminal_id": self.cfg.terminal_id,
            "reason": reason,
        });
        let _ = self
            .client
            .post(&url)
            .header("Authorization", self.cfg.auth_header())
            .json(&body)
            .send()
            .await;
        self.session = None;
        self.ux = if self.sense.filter().present() {
            EdgeUx::IdlePresent
        } else {
            EdgeUx::IdleEmpty
        };
        info!(?reason, "session ended");
        Ok(())
    }

    pub async fn sync_active(&mut self) -> anyhow::Result<()> {
        let url = format!(
            "{}/v1/sessions/active?dog_id={}&terminal_id={}",
            self.cfg.api_base.trim_end_matches('/'),
            self.cfg.dog_id,
            self.cfg.terminal_id
        );
        let res = self
            .client
            .get(&url)
            .header("Authorization", self.cfg.auth_header())
            .send()
            .await?
            .error_for_status()?;
        let sess: Option<SessionRecord> = res.json().await?;
        if let Some(s) = sess {
            self.session = Some(s.clone());
            self.ux = if s.state == SessionState::Active {
                EdgeUx::InSession
            } else {
                EdgeUx::Negotiating
            };
            if self.ux == EdgeUx::Negotiating {
                let _ = self.report_media_ready(true).await;
            }
        }
        Ok(())
    }

    pub async fn handle_intent(&mut self, intent: Intent) -> anyhow::Result<()> {
        match intent {
            Intent::Call | Intent::Play
                if matches!(self.ux, EdgeUx::IdlePresent) && self.social_armed =>
            {
                self.call(None).await?;
            }
            Intent::Call | Intent::Play | Intent::Again | Intent::Done
                if self.ux == EdgeUx::RingingIn =>
            {
                self.accept_incoming().await?;
            }
            Intent::Again if self.ux == EdgeUx::InSession => {
                self.again().await?;
            }
            Intent::Done if matches!(self.ux, EdgeUx::InSession | EdgeUx::Negotiating) => {
                self.end_session(EndReason::Done).await?;
            }
            _ => debug!(?intent, ux = ?self.ux, "ignored"),
        }
        Ok(())
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

#[allow(dead_code)]
fn _w() {
    warn!("x");
}
