use std::time::{Duration, Instant};

use crate::data::{self, Bulletin};

pub(super) struct DemoState {
    pub(super) frames: Vec<Bulletin>,
    pub(super) hidden_areas: Vec<Vec<String>>,
    pub(super) delay: Duration,
    pub(super) next: Option<Instant>,
    pub(super) bell: bool,
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
        }
    }

    pub(super) fn announce(&mut self, previous: Option<usize>, index: usize) {
        let previous = previous.map(|previous| &self.frames[previous]);
        self.bell |= data::demo_bell(previous, &self.frames[index]);
    }
}
