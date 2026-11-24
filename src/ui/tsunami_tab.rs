use super::App;
use crate::{
    data::{self, Provider, TsunamiStatus},
    map::View,
    tsunami::{State, Validity},
};

pub(super) struct TsunamiTab {
    pub(super) view: View,
    pub(super) state: State,
    pub(super) error: Option<String>,
    pub(super) received: bool,
}

impl Default for TsunamiTab {
    fn default() -> Self {
        Self {
            view: View::new(true),
            state: State::default(),
            error: None,
            received: false,
        }
    }
}

impl App {
    pub(super) fn confirmed_tsunami(&self) -> Option<&data::Tsunami> {
        self.tsunami
            .state
            .verified(self.provider)
            .filter(|report| report.status != TsunamiStatus::Clear || self.has_live_wolfx_tsunami())
    }

    fn has_live_wolfx_tsunami(&self) -> bool {
        self.connected == Some(true)
            && self.provider == Provider::Wolfx
            && self.tsunami.received
            && self.tsunami.error.is_none()
    }

    pub(super) fn tsunami_report_expired(&self) -> bool {
        self.has_live_wolfx_tsunami()
            && self.tsunami.state.validity == Validity::Current
            && self
                .tsunami
                .state
                .wolfx
                .as_ref()
                .is_some_and(|report| report.status == TsunamiStatus::Expired)
    }

    pub(super) fn tsunami_map_areas(&self) -> &[data::TsunamiArea] {
        self.confirmed_tsunami()
            .filter(|report| {
                matches!(
                    report.status,
                    TsunamiStatus::Alert | TsunamiStatus::Forecast
                )
            })
            .map_or(&[], |report| report.areas.as_slice())
    }

    pub(super) fn tsunami_display(&self) -> (&'static str, bool, Vec<String>, Vec<String>) {
        let mut notes = Vec::new();
        let mut areas = Vec::new();
        if let Some(last) = self.tsunami.state.last_issued {
            notes.push(format!("最後の発表 {} JST", last.format("%m/%d %H:%M")));
        }
        let confirmed = self.confirmed_tsunami();
        let status = match confirmed {
            Some(report) if report.status == TsunamiStatus::Alert => {
                areas.extend(report.areas.iter().map(data::TsunamiArea::line));
                "津波警報・注意報 発表中"
            }
            Some(report) if report.status == TsunamiStatus::Forecast => {
                areas.extend(report.areas.iter().map(data::TsunamiArea::line));
                "津波予報 発表中"
            }
            Some(_) => "現在、津波の発表はありません",
            None if self.tsunami_report_expired() => "津波の発表なし",
            None if self.tsunami.state.wolfx.is_none() && self.connected.is_none() => {
                "津波情報を取得中…"
            }
            None => "現在の津波の発表状況は未確認",
        };
        if self.provider == Provider::P2p
            && let Some(report) = &self.tsunami.state.reference
            && confirmed.is_none()
        {
            let label = if report.status == (TsunamiStatus::Reference { cancelled: true }) {
                "参考 · P2Pの解除報"
            } else {
                "参考 · P2Pが受信した発表内容"
            };
            areas.push(label.into());
            areas.extend(report.areas.iter().map(data::TsunamiArea::line));
        }
        if let Some(report) = &self.tsunami.state.wolfx {
            if report.status == TsunamiStatus::Expired {
                notes.push("最後の発表の有効期限切れ".into());
            }
            if self.tsunami.state.validity == Validity::Expired {
                notes.push("配信確認期限を過ぎています".into());
            }
            if let Some(until) = report.valid_until {
                notes.push(format!(
                    "Wolfx情報の確認期限 {} JST",
                    until.format("%m/%d %H:%M:%S")
                ));
            }
        }
        if let Some(error) = &self.tsunami.error {
            notes.push(format!("津波情報の取得・読み取りに失敗 · {error}"));
        }
        let alert = confirmed.is_some_and(data::Tsunami::alert);
        (status, alert, areas, notes)
    }
}
