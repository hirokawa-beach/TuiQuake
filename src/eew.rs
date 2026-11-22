use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[cfg(test)]
use chrono::DateTime;

use crate::data::{Bulletin, ServerClock, Stamp};

const ACTIVE_FOR: chrono::TimeDelta = chrono::TimeDelta::seconds(120);
const FUTURE_TOLERANCE: chrono::TimeDelta = chrono::TimeDelta::seconds(30);

struct ActiveReport {
    bulletin: Box<Bulletin>,
    expires: Instant,
}

fn report_order(report: &Bulletin) -> (Option<Stamp>, Option<u64>, bool, bool) {
    (
        report.issued,
        report.eew_serial,
        report.eew_cancelled,
        report.warning,
    )
}

#[derive(Default)]
pub struct State {
    clock: Option<ServerClock>,
    waiting: Option<Box<Bulletin>>,
    active: Option<ActiveReport>,
    last: Option<(Option<Stamp>, Option<u64>, bool, bool)>,
    seen: VecDeque<String>,
    notice: Option<String>,
}

impl State {
    pub fn active(&self) -> Option<&Bulletin> {
        self.active.as_ref().map(|report| report.bulletin.as_ref())
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn matches(&self, id: &str) -> bool {
        self.seen.iter().any(|event| event == id)
    }

    pub fn disconnect(&mut self) {
        self.clock = None;
    }

    pub fn switch_source(&mut self) {
        self.disconnect();
        self.waiting = None;
        self.notice = None;
    }

    pub fn sync(&mut self, clock: ServerClock, now: Instant) -> bool {
        self.clock = Some(clock);
        self.waiting
            .take()
            .is_some_and(|report| self.receive(report, now))
    }

    pub fn invalid(&mut self, reason: &str) -> bool {
        self.note(format!("EEWを読み取れません · {reason}"))
    }

    pub fn receive(&mut self, report: Box<Bulletin>, now: Instant) -> bool {
        let order = report_order(&report);
        if self.last.is_some_and(|prior| prior >= order)
            || self
                .waiting
                .as_ref()
                .is_some_and(|prior| report_order(prior) >= order)
        {
            return false;
        }
        let Some(issued) = report.issued else {
            return self.invalid("発表時刻がありません");
        };
        let stamp = issued.format("%m/%d %H:%M:%S");
        let Some(clock) = self.clock else {
            let message = format!(
                "EEW受信 · {}の時刻を待っています（発表 {stamp} JST）",
                report.provider.label()
            );
            self.waiting = Some(report);
            return self.note(message);
        };
        let age = clock.at(now).signed_duration_since(issued);
        if age >= ACTIVE_FOR {
            return self.note(format!(
                "EEWを受信 · 発表から120秒以上経過（発表 {stamp} JST）"
            ));
        }
        if age < -FUTURE_TOLERANCE {
            return self.note(format!(
                "EEWを受信 · 発表時刻が配信元の時刻より先です（発表 {stamp} JST）"
            ));
        }
        let remaining = (ACTIVE_FOR - age.max(chrono::TimeDelta::zero()))
            .to_std()
            .unwrap_or(Duration::ZERO);
        self.last = Some(order);
        self.record_event(&report);
        self.active = Some(ActiveReport {
            bulletin: report,
            expires: now + remaining,
        });
        self.notice = None;
        true
    }

    pub fn expire(&mut self, now: Instant) -> bool {
        if self
            .active
            .as_ref()
            .is_some_and(|report| now >= report.expires)
        {
            self.active = None;
            if self.waiting.is_none() {
                self.note("最後に受信したEEWは表示時間を過ぎました".into());
            }
            true
        } else {
            false
        }
    }

    fn note(&mut self, message: String) -> bool {
        if self.notice.as_ref() == Some(&message) {
            return false;
        }
        self.notice = Some(message);
        true
    }

    fn record_event(&mut self, report: &Bulletin) {
        let Some(id) = &report.eew_event_id else {
            return;
        };
        if !self.seen.contains(id) {
            if self.seen.len() >= 256 {
                self.seen.pop_front();
            }
            self.seen.push_back(id.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(received: Instant) -> ServerClock {
        let utc = DateTime::parse_from_rfc3339("2026-11-22T10:20:00Z")
            .unwrap()
            .to_utc();
        ServerClock { utc, received }
    }

    fn report(clock: ServerClock, age_seconds: i64) -> Box<Bulletin> {
        Box::new(Bulletin {
            eew: true,
            eew_event_id: Some("fixture".into()),
            eew_serial: Some(1),
            issued: Some((clock.utc - chrono::TimeDelta::seconds(age_seconds)).fixed_offset()),
            ..Bulletin::default()
        })
    }

    #[test]
    fn waits_for_wolfx_time_and_expires_using_monotonic_elapsed_time() {
        let received = Instant::now();
        let clock = clock(received);
        let mut state = State::default();
        assert!(state.receive(report(clock, 20), received));
        assert!(state.active().is_none());
        assert!(state.notice().unwrap().contains("時刻を待っています"));
        assert!(state.sync(clock, received));
        assert!(state.active().is_some());
        state.disconnect();
        assert!(!state.expire(received + Duration::from_secs(99)));
        assert!(state.expire(received + Duration::from_secs(100)));
        assert!(state.active().is_none());
        assert!(state.notice().unwrap().contains("表示時間を過ぎました"));
    }

    #[test]
    fn stale_or_inconsistent_timestamps_are_visible_and_silent() {
        let now = Instant::now();
        let clock = clock(now);
        let mut state = State::default();
        state.sync(clock, now);
        for (age, reason) in [(121, "120秒以上"), (-31, "より先")] {
            assert!(state.receive(report(clock, age), now));
            assert!(state.active().is_none());
            assert!(state.notice().unwrap().contains(reason));
        }
    }
}
