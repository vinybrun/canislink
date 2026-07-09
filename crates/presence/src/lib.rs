//! Presence fusion: mat force + ToF + motion → debounced present flag.
//!
//! Architecture defaults:
//! - enter debounce 800 ms
//! - exit debounce 2500 ms
//! - present_raw = (F > F_min) AND (ToF in band OR motion)

use protocol::{ForceBand, PRESENCE_ENTER_MS, PRESENCE_EXIT_MS};

/// Tunable thresholds (per terminal / dog size class later).
#[derive(Debug, Clone)]
pub struct PresenceConfig {
    pub force_min_n: f32,
    pub tof_near_mm: u16,
    pub tof_far_mm: u16,
    pub enter_ms: u64,
    pub exit_ms: u64,
}

impl Default for PresenceConfig {
    fn default() -> Self {
        Self {
            force_min_n: 25.0,
            tof_near_mm: 50,
            tof_far_mm: 900,
            enter_ms: PRESENCE_ENTER_MS,
            exit_ms: PRESENCE_EXIT_MS,
        }
    }
}

/// One raw sensor observation.
#[derive(Debug, Clone, Copy)]
pub struct SenseSample {
    pub force_n: f32,
    pub tof_mm: Option<u16>,
    pub motion: bool,
    /// Monotonic ms clock (edge local).
    pub t_ms: u64,
}

/// Debounced presence state machine.
#[derive(Debug, Clone)]
pub struct PresenceFilter {
    pub cfg: PresenceConfig,
    present: bool,
    raw: bool,
    raw_since_ms: u64,
    last_t_ms: u64,
    confidence: f32,
    last_force_n: f32,
    last_tof: Option<u16>,
}

impl Default for PresenceFilter {
    fn default() -> Self {
        Self::new(PresenceConfig::default())
    }
}

impl PresenceFilter {
    pub fn new(cfg: PresenceConfig) -> Self {
        Self {
            cfg,
            present: false,
            raw: false,
            raw_since_ms: 0,
            last_t_ms: 0,
            confidence: 0.0,
            last_force_n: 0.0,
            last_tof: None,
        }
    }

    pub fn present(&self) -> bool {
        self.present
    }

    pub fn confidence(&self) -> f32 {
        self.confidence
    }

    pub fn force_band(&self) -> ForceBand {
        ForceBand::from_newtons(self.last_force_n)
    }

    pub fn last_force_n(&self) -> f32 {
        self.last_force_n
    }

    pub fn last_tof_mm(&self) -> Option<u16> {
        self.last_tof
    }

    fn raw_present(&self, s: &SenseSample) -> bool {
        let force_ok = s.force_n >= self.cfg.force_min_n;
        let tof_ok = s
            .tof_mm
            .map(|d| d >= self.cfg.tof_near_mm && d <= self.cfg.tof_far_mm)
            .unwrap_or(false);
        force_ok && (tof_ok || s.motion)
    }

    fn confidence_of(&self, s: &SenseSample, raw: bool) -> f32 {
        if !raw {
            return 0.05;
        }
        let mut c: f32 = 0.4;
        if s.force_n >= self.cfg.force_min_n {
            c += 0.35;
        }
        if s.tof_mm
            .map(|d| d >= self.cfg.tof_near_mm && d <= self.cfg.tof_far_mm)
            .unwrap_or(false)
        {
            c += 0.2;
        }
        if s.motion {
            c += 0.05;
        }
        c.clamp(0.0, 1.0)
    }

    /// Ingest a sample; returns true if debounced `present` flipped.
    pub fn update(&mut self, s: SenseSample) -> bool {
        self.last_t_ms = s.t_ms;
        self.last_force_n = s.force_n;
        self.last_tof = s.tof_mm;

        let raw = self.raw_present(&s);
        self.confidence = self.confidence_of(&s, raw);

        if raw != self.raw {
            self.raw = raw;
            self.raw_since_ms = s.t_ms;
        }

        let held = s.t_ms.saturating_sub(self.raw_since_ms);
        let prev = self.present;

        if !self.present && self.raw && held >= self.cfg.enter_ms {
            self.present = true;
        } else if self.present && !self.raw && held >= self.cfg.exit_ms {
            self.present = false;
        }

        prev != self.present
    }
}

/// Cloud-side presence store entry evaluation.
pub fn still_online(last_seen_ms: u64, now_ms: u64, ttl_ms: u64) -> bool {
    now_ms.saturating_sub(last_seen_ms) <= ttl_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(t: u64, force: f32, tof: u16, motion: bool) -> SenseSample {
        SenseSample {
            force_n: force,
            tof_mm: Some(tof),
            motion,
            t_ms: t,
        }
    }

    #[test]
    fn enter_requires_debounce() {
        let mut f = PresenceFilter::default();
        // below enter window
        f.update(sample(0, 100.0, 400, false));
        assert!(!f.present());
        f.update(sample(400, 100.0, 400, false));
        assert!(!f.present());
        f.update(sample(800, 100.0, 400, false));
        assert!(f.present());
    }

    #[test]
    fn exit_requires_longer_debounce() {
        let mut f = PresenceFilter::default();
        f.update(sample(0, 100.0, 400, false));
        f.update(sample(800, 100.0, 400, false));
        assert!(f.present());

        f.update(sample(900, 0.0, 0, false));
        assert!(f.present());
        f.update(sample(900 + 2499, 0.0, 0, false));
        assert!(f.present());
        f.update(sample(900 + 2500, 0.0, 0, false));
        assert!(!f.present());
    }

    #[test]
    fn force_alone_insufficient_without_tof_or_motion() {
        let mut f = PresenceFilter::default();
        f.update(sample(0, 100.0, 5000, false)); // tof out of band, no motion
        f.update(sample(1000, 100.0, 5000, false));
        assert!(!f.present());

        f.update(sample(2000, 100.0, 5000, true)); // motion saves it
        f.update(sample(2800, 100.0, 5000, true));
        assert!(f.present());
    }
}
