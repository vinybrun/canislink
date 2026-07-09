//! Edge agent library — presence reporting loop.

use canis_sense::{SensePipeline, SenseSnapshot};
use chrono::Utc;
use protocol::{DogId, PresenceReport, TerminalId, PRESENCE_PUBLISH_MS};
use reqwest::Client;
use std::time::Duration;
use tracing::{debug, warn};

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

#[derive(Debug)]
pub struct EdgeAgent {
    pub cfg: EdgeConfig,
    pub sense: SensePipeline,
    client: Client,
    seq: u64,
    last_published_present: Option<bool>,
}

impl EdgeAgent {
    pub fn new(cfg: EdgeConfig) -> Self {
        Self {
            cfg,
            sense: SensePipeline::new(),
            client: Client::new(),
            seq: 0,
            last_published_present: None,
        }
    }

    pub fn ingest_sample(
        &mut self,
        force_n: f32,
        tof_mm: Option<u16>,
        motion: bool,
        dt_ms: u64,
    ) -> SenseSnapshot {
        self.sense.push_sample(force_n, tof_mm, motion, dt_ms)
    }

    pub fn ingest_uart(&mut self, bytes: &[u8], dt_ms: u64) -> Vec<SenseSnapshot> {
        self.sense.push_bytes(bytes, dt_ms)
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

    /// Publish if flipped or periodic while present / after leave.
    pub async fn maybe_publish(&mut self, force: bool) -> anyhow::Result<bool> {
        let present = self.sense.filter().present();
        let flipped = self.last_published_present != Some(present);
        if !force && !flipped && !present {
            // offline quiet
            return Ok(false);
        }
        if !force && !flipped && present {
            // periodic handled by caller timer — if force false and not flipped, skip
            // caller sets force=true on publish interval
            return Ok(false);
        }
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
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            warn!(%status, %body, "presence publish failed");
            anyhow::bail!("presence publish failed: {status}");
        }
        self.last_published_present = Some(report.present);
        debug!(
            present = report.present,
            seq = report.seq,
            "presence published"
        );
        Ok(true)
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

    pub fn publish_interval() -> Duration {
        Duration::from_millis(PRESENCE_PUBLISH_MS)
    }
}
