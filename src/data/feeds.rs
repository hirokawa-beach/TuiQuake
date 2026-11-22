use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use super::{Client, LiveQuery, LiveUpdate, Provider, p2p, wolfx};

struct Feed {
    requests: mpsc::Sender<LiveQuery>,
    updates: mpsc::Receiver<LiveUpdate>,
}

impl Feed {
    fn start(client: Client, provider: Provider) -> Self {
        let (requests, queries) = mpsc::channel();
        let (send, updates) = mpsc::channel();
        thread::spawn(move || {
            let mut publish = |update| send.send(update).is_ok();
            let mut retry = Duration::from_secs(5);
            loop {
                if matches!(queries.try_recv(), Err(mpsc::TryRecvError::Disconnected)) {
                    break;
                }
                let started = Instant::now();
                let error = match match provider {
                    Provider::Wolfx => wolfx::live_connection(&queries, &mut publish),
                    Provider::P2p => p2p::connect(&client, &queries, &mut publish),
                } {
                    Ok(()) => break,
                    Err(error) => error,
                };
                if !publish(LiveUpdate::Connection(provider, Some(error.to_string()))) {
                    break;
                }
                if started.elapsed() >= Duration::from_secs(60) {
                    retry = Duration::from_secs(5);
                }
                let until = Instant::now() + retry;
                // Discard queries during retry. Every new connection loads its initial data.
                while Instant::now() < until {
                    if matches!(
                        queries.recv_timeout(until.saturating_duration_since(Instant::now())),
                        Err(mpsc::RecvTimeoutError::Disconnected)
                    ) {
                        return;
                    }
                }
                retry = (retry * 2).min(Duration::from_secs(60));
            }
        });
        Self { requests, updates }
    }
}

fn supervise(
    primary: Feed,
    mut start_backup: impl FnMut() -> Feed,
    requests: mpsc::Receiver<LiveQuery>,
    notify: &mut dyn FnMut(LiveUpdate) -> bool,
) {
    let mut backup: Option<Feed> = None;
    loop {
        loop {
            match requests.try_recv() {
                Ok(query) => {
                    let _ = backup.as_ref().unwrap_or(&primary).requests.send(query);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
        match primary.updates.recv_timeout(Duration::from_millis(50)) {
            Ok(update @ LiveUpdate::Connection(Provider::Wolfx, None)) => {
                backup = None;
                if !notify(update) {
                    return;
                }
            }
            Ok(LiveUpdate::Connection(Provider::Wolfx, Some(error))) => {
                if backup.is_none() {
                    if !notify(LiveUpdate::Connection(
                        Provider::P2p,
                        Some(format!("Wolfx: {error}")),
                    )) {
                        return;
                    }
                    backup = Some(start_backup());
                }
            }
            Ok(update) if backup.is_none() => {
                if !notify(update) {
                    return;
                }
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if let Some(feed) = &backup {
            for update in feed.updates.try_iter() {
                if !notify(update) {
                    return;
                }
            }
        }
    }
}

pub(super) fn stream(
    client: &Client,
    requests: mpsc::Receiver<LiveQuery>,
    notify: &mut dyn FnMut(LiveUpdate) -> bool,
) {
    supervise(
        Feed::start(client.clone(), Provider::Wolfx),
        || Feed::start(client.clone(), Provider::P2p),
        requests,
        notify,
    );
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn feed() -> (Feed, mpsc::Sender<LiveUpdate>) {
        let (requests, _) = mpsc::channel();
        let (send, updates) = mpsc::channel();
        (Feed { requests, updates }, send)
    }

    #[test]
    fn uses_backup_only_after_primary_failure_and_returns_when_ready() {
        let (primary, wolfx) = feed();
        let (backup, p2p) = feed();
        let (requests, queries) = mpsc::channel();
        let (send, outputs) = mpsc::channel();
        let mut backup = Some(backup);
        let worker = thread::spawn(move || {
            supervise(
                primary,
                || backup.take().expect("backup starts once"),
                queries,
                &mut |update| send.send(update).is_ok(),
            )
        });
        let next = || outputs.recv_timeout(Duration::from_secs(2)).unwrap();
        wolfx
            .send(LiveUpdate::Connection(Provider::Wolfx, None))
            .unwrap();
        assert!(matches!(
            next(),
            LiveUpdate::Connection(Provider::Wolfx, None)
        ));
        wolfx
            .send(LiveUpdate::Connection(
                Provider::Wolfx,
                Some("offline".into()),
            ))
            .unwrap();
        assert!(matches!(
            next(),
            LiveUpdate::Connection(Provider::P2p, Some(_))
        ));
        p2p.send(LiveUpdate::Connection(Provider::P2p, None))
            .unwrap();
        assert!(matches!(
            next(),
            LiveUpdate::Connection(Provider::P2p, None)
        ));
        wolfx
            .send(LiveUpdate::Connection(Provider::Wolfx, None))
            .unwrap();
        assert!(matches!(
            next(),
            LiveUpdate::Connection(Provider::Wolfx, None)
        ));
        assert!(p2p.send(LiveUpdate::Quakes(Arc::default())).is_err());
        drop(requests);
        worker.join().unwrap();
    }
}
