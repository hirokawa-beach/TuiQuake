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
    cancelled: bool,
    warning_issued: Option<Stamp>,
    warning_areas: Vec<String>,
    last: Option<ReportOrder>,
    active: Option<ActiveReport>,
}

type ReportOrder = (Option<Stamp>, Option<u64>, bool, bool);

fn report_order(report: &Bulletin) -> ReportOrder {
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
    waiting: Vec<Bulletin>,
    seen: VecDeque<ReceivedEvent>,
    notice: Option<(String, bool)>,
    latest: Option<(Stamp, bool)>,
    bell: bool,
}

impl State {
    pub fn reports(&self) -> impl DoubleEndedIterator<Item = &Bulletin> {
        self.seen
            .iter()
            .filter_map(|event| event.active.as_ref().map(|report| report.bulletin.as_ref()))
    }

    pub fn active(&self) -> Option<&Bulletin> {
        self.reports().next_back()
    }

    pub fn latest_report(&self) -> Option<&Bulletin> {
        self.reports().max_by_key(|report| report_order(report))
    }

    pub fn alert(&self) -> Option<&Bulletin> {
        self.reports()
            .rfind(|report| report.has_eew_warning())
            .or_else(|| self.reports().rfind(|report| !report.eew_cancelled))
            .or_else(|| self.active())
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

    pub fn warning_seconds(&self, id: &str, now: Instant) -> Option<u64> {
        let report = self
            .seen
            .iter()
            .find(|event| event.id == id)?
            .active
            .as_ref()?;
        let issued = report.bulletin.issued?;
        let warning = report.bulletin.eew_warning_issued?;
        let remaining =
            chrono::TimeDelta::from_std(report.expires.saturating_duration_since(now)).ok()?;
        let age = ACTIVE_FOR - remaining;
        Some((age + (issued - warning)).num_seconds().max(0) as u64)
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
        self.waiting.clear();
        self.notice = None;
    }

    pub fn sync(&mut self, clock: ServerClock, now: Instant) -> bool {
        self.clock = Some(clock);
        let mut waiting = std::mem::take(&mut self.waiting);
        waiting.sort_by_key(report_order);
        let mut changed = false;
        for report in waiting {
            changed |= self.receive(Box::new(report), now);
        }
        changed
    }

    pub fn invalid(&mut self, reason: &str) -> bool {
        self.note(format!("EEWを読み取れません · {reason}"), true)
    }

    pub fn receive(&mut self, mut report: Box<Bulletin>, now: Instant) -> bool {
        let Some(id) = report.eew_event_id.as_deref() else {
            return self.invalid("地震IDがありません");
        };
        let Some(issued) = report.issued else {
            return self.invalid("発表時刻がありません");
        };
        let order = report_order(&report);
        let previous = self.seen.iter().position(|event| event.id == id);
        let event = previous.map(|index| &self.seen[index]);
        if !report.eew_cancelled && event.is_some_and(|event| event.cancelled) {
            return false;
        }
        let newer = event
            .and_then(|event| event.last)
            .is_none_or(|prior| prior < order);
        let late_warning =
            !newer && event.is_some_and(|event| event.active.is_some()) && report.has_eew_warning();
        let known_cancellation =
            report.eew_cancelled && event.is_some_and(|event| !event.cancelled);
        if !newer && !late_warning && !known_cancellation {
            return false;
        }
        let stamp = issued.format("%m/%d %H:%M:%S");
        let Some(clock) = self.clock else {
            let message = format!(
                "EEW受信 · {}の時刻を待っています（発表 {stamp} JST）",
                report.provider.label()
            );
            if self.waiting.len() == 64 {
                self.waiting.remove(0);
            }
            self.waiting.push(*report);
            return self.note(message, true);
        };
        let age = clock.at(now).signed_duration_since(issued);
        if age >= ACTIVE_FOR && !known_cancellation {
            if !newer {
                return false;
            }
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
        let index = match previous {
            Some(index) => index,
            None => {
                if self.seen.len() >= 256
                    && let Some(inactive) =
                        self.seen.iter().position(|event| event.active.is_none())
                {
                    self.seen.remove(inactive);
                }
                self.seen.push_back(ReceivedEvent {
                    id: id.to_owned(),
                    announced: false,
                    cancelled: false,
                    warning_issued: None,
                    warning_areas: Vec::new(),
                    last: None,
                    active: None,
                });
                self.seen.len() - 1
            }
        };
        let event = &mut self.seen[index];
        event.last = event.last.max(Some(order));
        Self::record_event(event, &mut report, issued, &mut self.bell);
        if late_warning && let Some(active) = &mut event.active {
            let active = &mut active.bulletin;
            let changed = active.eew_warning_issued != report.eew_warning_issued
                || active.warning_areas != report.warning_areas;
            active.eew_warning_issued = report.eew_warning_issued;
            active.warning_areas = report.warning_areas;
            return changed;
        }
        if !newer && report.eew_cancelled && event.active.is_none() {
            return false;
        }
        if report.eew_cancelled
            && report.name.is_empty()
            && let Some(active) = &event.active
        {
            report.name.clone_from(&active.bulletin.name);
        }
        let remaining = (ACTIVE_FOR - age.max(chrono::TimeDelta::zero()))
            .to_std()
            .unwrap_or(Duration::ZERO);
        event.active = Some(ActiveReport {
            bulletin: report,
            expires: now + remaining,
        });
        self.remember(issued, false);
        self.notice = None;
        true
    }

    pub fn expire(&mut self, now: Instant) -> bool {
        let mut expired = None;
        for event in &mut self.seen {
            if event
                .active
                .as_ref()
                .is_some_and(|report| now >= report.expires)
                && let Some(report) = event.active.take()
            {
                expired = expired.max(report.bulletin.issued);
            }
        }
        let Some(issued) = expired else {
            return false;
        };
        self.remember(issued, true);
        if self.active().is_none()
            && self.waiting.is_empty()
            && self.notice.as_ref().is_none_or(|(_, uncertain)| !uncertain)
        {
            self.note("最後に受信したEEWは表示時間を過ぎました".into(), false);
        }
        true
    }

    fn note(&mut self, message: String, uncertain: bool) -> bool {
        let notice = (message, uncertain);
        if self.notice.as_ref() == Some(&notice) {
            return false;
        }
        self.notice = Some(notice);
        true
    }

    fn record_event(
        event: &mut ReceivedEvent,
        report: &mut Bulletin,
        issued: Stamp,
        bell: &mut bool,
    ) {
        if report.eew_cancelled {
            event.cancelled = true;
            event.warning_issued = None;
            event.warning_areas.clear();
        } else {
            let warned = report.has_eew_warning();
            let already_warned = event.warning_issued.is_some() || !event.warning_areas.is_empty();
            *bell |= !event.announced || warned && !already_warned;
            event.announced = true;
            if report.warning {
                event.warning_issued = Some(
                    event
                        .warning_issued
                        .map_or(issued, |prior| prior.max(issued)),
                );
            }
            // 警報の続報は対象地域を追加する。予報の震度変化では取り消さない。
            for name in &report.warning_areas {
                if !event.warning_areas.contains(name) {
                    event.warning_areas.push(name.clone());
                }
            }
        }
        report.eew_warning_issued = event.warning_issued;
        report.warning_areas.clone_from(&event.warning_areas);
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
            state.warning_seconds("fixture", now + Duration::from_secs(7)),
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

    #[test]
    fn overlapping_reports_accept_older_warnings_and_expire_independently() {
        let now = Instant::now();
        let clock = clock(now);
        let mut state = State::default();
        state.sync(clock, now);
        let mut forecast = report(clock, 2);
        forecast.eew_event_id = Some("other".into());
        assert!(state.receive(forecast, now));
        let mut warning = report(clock, 5);
        warning.warning = true;
        warning.warning_areas = vec!["熊本県熊本".into()];
        assert!(state.receive(warning, now));
        assert_eq!(state.reports().count(), 2);
        assert_eq!(state.alert().unwrap().warning_areas, ["熊本県熊本"]);
        assert!(state.expire(now + Duration::from_secs(115)));
        assert_eq!(state.reports().count(), 1);
        assert_eq!(
            state.active().unwrap().eew_event_id.as_deref(),
            Some("other")
        );
        assert!(state.expire(now + Duration::from_secs(118)));
        assert!(state.active().is_none());
    }

    #[test]
    fn warning_areas_survive_forecasts_and_follow_the_event_id() {
        let now = Instant::now();
        let clock = clock(now);
        let mut state = State::default();
        let mut warning = report(clock, 8);
        warning.warning = true;
        warning.warning_areas = vec!["熊本県熊本".into()];
        let mut forecast = report(clock, 5);
        forecast.eew_final = true;
        forecast.maximum = Some(crate::data::Level::Known(2));
        // Both packets may arrive before the server clock is ready.
        assert!(state.receive(forecast.clone(), now));
        state.receive(warning, now);
        assert!(state.sync(clock, now));
        let active = state.active().unwrap();
        assert!(active.eew_final && !active.warning && active.has_eew_warning());
        assert_eq!(active.warning_areas, ["熊本県熊本"]);
        assert_eq!(active.maximum.as_ref().unwrap().label(), "3");
        assert_eq!(state.warning_seconds("fixture", now), Some(8));

        // A warning from the fallback may arrive out of order and list just the addition.
        let mut addition = report(clock, 6);
        addition.provider = crate::data::Provider::P2p;
        addition.warning = true;
        addition.warning_areas = vec!["熊本県天草・芦北".into()];
        assert!(state.receive(addition.clone(), now));
        assert!(!state.receive(addition, now));
        let active = state.active().unwrap();
        assert_eq!(active.warning_areas, ["熊本県熊本", "熊本県天草・芦北"]);
        assert!(active.eew_final);
        assert_eq!(active.provider, crate::data::Provider::Wolfx);

        let mut other = report(clock, 4);
        other.eew_event_id = Some("other".into());
        assert!(state.receive(other, now));
        assert!(!state.active().unwrap().has_eew_warning());
        assert_eq!(state.reports().count(), 2);
        assert_eq!(state.alert().unwrap().warning_areas.len(), 2);

        forecast.issued = report(clock, 3).issued;
        assert!(state.receive(forecast.clone(), now));
        assert_eq!(state.alert().unwrap().warning_areas.len(), 2);
        assert_eq!(
            state.active().unwrap().eew_event_id.as_deref(),
            Some("other")
        );
        let mut cancellation = report(clock, 6);
        cancellation.eew_serial = None;
        cancellation.eew_cancelled = true;
        assert!(state.receive(cancellation, now));
        let cancelled = state.reports().next().unwrap();
        assert!(cancelled.eew_cancelled && cancelled.warning_areas.is_empty());
        assert_eq!(
            state.alert().unwrap().eew_event_id.as_deref(),
            Some("other")
        );
        forecast.issued = report(clock, 1).issued;
        assert!(!state.receive(forecast, now));
    }
}
