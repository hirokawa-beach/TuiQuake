use std::time::{Duration, Instant};

use crate::data::{self, Bulletin};

pub(super) struct DemoState {
    pub(super) frames: Vec<Bulletin>,
    pub(super) hidden_areas: Vec<Vec<String>>,
    pub(super) delay: Duration,
    pub(super) next: Option<Instant>,
    pub(super) bell: bool,
    wave_elapsed: Duration,
}

impl DemoState {
    pub(super) fn new(delay: Duration) -> Self {
        let frames = data::demo();
        Self {
            hidden_areas: vec![Vec::new(); frames.len()],
            frames,
            delay: delay.max(Duration::from_millis(150)),
            next: None,
            bell: false,
            wave_elapsed: Duration::ZERO,
        }
    }

    pub(super) fn tick_wave(&mut self, now: Instant, index: usize) -> bool {
        let Some(next) = self.next else {
            return false;
        };
        if !self.frames[index].eew {
            return false;
        }
        let elapsed = self
            .delay
            .saturating_sub(next.saturating_duration_since(now));
        let gap = self.frames[index]
            .issued
            .zip(
                self.frames
                    .get(index + 1)
                    .and_then(|bulletin| bulletin.issued),
            )
            .and_then(|(issued, following)| (following - issued).to_std().ok())
            .unwrap_or_default();
        let elapsed = elapsed.min(gap);
        let tick = Duration::from_millis(250);
        let snapped = elapsed - Duration::from_nanos((elapsed.as_nanos() % tick.as_nanos()) as u64);
        if self.wave_elapsed == snapped {
            return false;
        }
        self.wave_elapsed = snapped;
        true
    }

    pub(super) fn wave_seconds(&self, bulletin: &Bulletin) -> Option<f64> {
        if !bulletin.eew || bulletin.eew_cancelled || bulletin.plum_assumed_hypocenter {
            return None;
        }
        bulletin.epicenter.zip(bulletin.depth)?;
        let elapsed = (bulletin.issued? - bulletin.origin?).to_std().ok()?;
        Some(elapsed.saturating_add(self.wave_elapsed).as_secs_f64())
    }

    pub(super) fn announce(&mut self, previous: Option<usize>, index: usize) {
        self.wave_elapsed = Duration::ZERO;
        let previous = previous.map(|previous| &self.frames[previous]);
        self.bell |= data::demo_bell(previous, &self.frames[index]);
    }
}
