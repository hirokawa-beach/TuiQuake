use std::time::Instant;

use crate::data::{Provider, ServerClock, Tsunami};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Validity {
    #[default]
    Unconfirmed,
    AwaitingClock,
    Current,
    Expired,
}

#[derive(Default)]
pub struct State {
    pub wolfx: Option<Tsunami>,
    pub reference: Option<Tsunami>,
    pub validity: Validity,
    wolfx_clock: Option<ServerClock>,
    expires: Option<Instant>,
}

impl State {
    pub fn receive(&mut self, report: Tsunami, now: Instant) {
        if report.provider == Provider::P2p {
            self.reference = Some(report);
        } else {
            if self.wolfx.as_ref().and_then(|old| old.valid_until) != report.valid_until {
                self.expires = None;
            }
            self.wolfx = Some(report);
            self.set_deadline(now);
        }
    }

    pub fn sync_wolfx(&mut self, clock: ServerClock, now: Instant) {
        self.wolfx_clock = Some(clock);
        self.set_deadline(now);
    }

    fn set_deadline(&mut self, now: Instant) {
        let Some((clock, report)) = self.wolfx_clock.zip(self.wolfx.as_ref()) else {
            return;
        };
        let Some(until) = report.valid_until else {
            return;
        };
        let server_now = clock.at(now);
        let deadline = if report.current(server_now) {
            let remaining = until
                .signed_duration_since(server_now)
                .to_std()
                .unwrap_or_default();
            let Some(deadline) = now.checked_add(remaining) else {
                return;
            };
            deadline
        } else {
            now
        };
        self.expires = Some(self.expires.map_or(deadline, |old| old.min(deadline)));
    }

    pub fn update(&mut self, now: Instant) -> bool {
        let next = match (
            self.wolfx.as_ref().and_then(|report| report.valid_until),
            self.expires,
        ) {
            (None, _) => Validity::Unconfirmed,
            (_, None) => Validity::AwaitingClock,
            (_, Some(deadline)) if now < deadline => Validity::Current,
            _ => Validity::Expired,
        };
        let changed = next != self.validity;
        self.validity = next;
        changed
    }

    pub fn verified(&self, provider: Provider) -> Option<&Tsunami> {
        let report = self
            .wolfx
            .as_ref()
            .filter(|_| self.validity == Validity::Current)?;
        if provider == Provider::P2p
            && self
                .reference
                .as_ref()
                .is_some_and(|reference| reference.issued > report.issued)
        {
            return None;
        }
        Some(report)
    }
}
