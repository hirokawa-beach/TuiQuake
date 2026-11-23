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

struct ReceivedEvent {
    id: String,
    announced: bool,
    warned: bool,
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
    seen: VecDeque<ReceivedEvent>,
    notice: Option<(String, bool)>,
    latest: Option<(Stamp, bool)>,
    bell: bool,
}

impl State {
    pub fn active(&self) -> Option<&Bulletin> {
        self.active.as_ref().map(|report| report.bulletin.as_ref())
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_ref().map(|(message, _)| message.as_str())
    }

    pub fn latest(&self) -> Option<(Stamp, bool)> {
        self.latest
    }

    pub fn can_confirm_idle(&self) -> bool {
        self.clock.is_some() && self.notice.as_ref().is_none_or(|(_, uncertain)| !uncertain)
    }

    pub fn warning_seconds(&self, now: Instant) -> Option<u64> {
        let report = self
            .active
            .as_ref()
            .filter(|report| report.bulletin.warning && !report.bulletin.eew_cancelled)?;
        Some(
            ACTIVE_FOR
                .to_std()
                .ok()?
                .saturating_sub(report.expires.saturating_duration_since(now))
                .as_secs(),
        )
    }

    fn remember(&mut self, issued: Stamp, expired: bool) {
        if self.latest.is_none_or(|(previous, _)| previous <= issued) {
            self.latest = Some((issued, expired));
        }
    }

    pub fn matches(&self, id: &str) -> bool {
        self.seen.iter().any(|event| event.id == id)
    }

    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
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
        self.note(format!("EEWを読み取れません · {reason}"), true)
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
            return self.note(message, true);
        };
        let age = clock.at(now).signed_duration_since(issued);
        if age >= ACTIVE_FOR {
            self.remember(issued, true);
            return self.note(
                format!("EEWを受信 · 発表から120秒以上経過（発表 {stamp} JST）"),
                false,
            );
        }
        if age < -FUTURE_TOLERANCE {
            return self.note(
                format!("EEWを受信 · 発表時刻が配信元の時刻より先です（発表 {stamp} JST）"),
                true,
            );
        }
        let remaining = (ACTIVE_FOR - age.max(chrono::TimeDelta::zero()))
            .to_std()
            .unwrap_or(Duration::ZERO);
        self.last = Some(order);
        self.remember(issued, false);
        self.record_event(&report);
        self.active = Some(ActiveReport {
            bulletin: report,
            expires: now + remaining,
        });
        self.notice = None;
        true
    }

    pub fn expire(&mut self, now: Instant) -> bool {
        if let Some(report) = &self.active
            && now >= report.expires
        {
            if let Some(issued) = report.bulletin.issued {
                self.remember(issued, true);
            }
            self.active = None;
            if self.waiting.is_none()
                && self.notice.as_ref().is_none_or(|(_, uncertain)| !uncertain)
            {
                self.note("最後に受信したEEWは表示時間を過ぎました".into(), false);
            }
            true
        } else {
            false
        }
    }

    fn note(&mut self, message: String, uncertain: bool) -> bool {
        let notice = (message, uncertain);
        if self.notice.as_ref() == Some(&notice) {
            return false;
        }
        self.notice = Some(notice);
        true
    }

    fn record_event(&mut self, report: &Bulletin) {
        let Some(id) = &report.eew_event_id else {
            return;
        };
        if let Some(event) = self.seen.iter_mut().find(|event| event.id == *id) {
            if !report.eew_cancelled && (!event.announced || report.warning && !event.warned) {
                self.bell = true;
                event.announced = true;
                event.warned |= report.warning;
            }
        } else {
            if self.seen.len() >= 256 {
                self.seen.pop_front();
            }
            self.seen.push_back(ReceivedEvent {
                id: id.clone(),
                announced: !report.eew_cancelled,
                warned: report.warning,
            });
            self.bell |= !report.eew_cancelled;
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
        assert!(!state.take_bell());
        assert!(state.sync(clock, received));
        assert!(state.active().is_some());
        assert!(state.take_bell());
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
            assert!(!state.take_bell());
        }
    }

    #[test]
    fn bells_are_deduplicated_with_one_more_on_warning_escalation() {
        let now = Instant::now();
        let clock = clock(now);
        let mut state = State::default();
        state.sync(clock, now);
        let mut update = report(clock, 3);
        assert!(state.receive(update.clone(), now));
        assert!(state.take_bell());
        assert!(!state.receive(update.clone(), now));
        assert!(!state.take_bell());
        update.eew_serial = Some(2);
        update.warning = true;
        assert!(state.receive(update.clone(), now));
        assert_eq!(
            state.warning_seconds(now + Duration::from_secs(7)),
            Some(10)
        );
        assert!(state.take_bell());
        update.eew_serial = Some(3);
        assert!(state.receive(update.clone(), now));
        assert!(!state.take_bell());
        update.eew_serial = Some(4);
        update.eew_cancelled = true;
        update.warning = false;
        assert!(state.receive(update, now));
        assert!(!state.take_bell());
        assert!(state.active().unwrap().eew_cancelled);
    }
}
