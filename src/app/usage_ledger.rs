//! Background owner of the append-only cost ledger (D9 in
//! `docs/dev/usage-ui-design.md`; format and delta logic in
//! `mercury_usage::ledger`).
//!
//! One thread per server owns the file and the per-session baselines, so
//! (a) no ledger I/O ever runs on the main loop, and (b) startup load and live
//! appends are strictly ordered: the thread reads the existing ledger first
//! and only then turns live cumulative reports into deltas, so a report that
//! arrives during startup can never be double counted against an empty
//! baseline. Results flow back as `AppEvent::UsageCostRecorded`.

use std::path::PathBuf;
use std::sync::mpsc;

use mercury_usage::{ledger, CostObservation};
use time::OffsetDateTime;

use crate::events::AppEvent;

pub(crate) struct CostLedgerHandle {
    tx: mpsc::Sender<CostObservation>,
}

impl CostLedgerHandle {
    /// Starts the ledger thread. `horizon_start` bounds which past entries are
    /// sent back to the app (baselines are always rebuilt from every line).
    pub(crate) fn spawn(
        path: PathBuf,
        horizon_start: OffsetDateTime,
        event_tx: tokio::sync::mpsc::Sender<AppEvent>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || run(path, horizon_start, rx, event_tx));
        Self { tx }
    }

    /// Fire-and-forget: the thread computes the delta and appends it.
    pub(crate) fn observe(&self, observation: CostObservation) {
        let _ = self.tx.send(observation);
    }
}

fn run(
    path: PathBuf,
    horizon_start: OffsetDateTime,
    rx: mpsc::Receiver<CostObservation>,
    event_tx: tokio::sync::mpsc::Sender<AppEvent>,
) {
    let (mut baselines, entries, persist) =
        match ledger::load(&path, |entry| entry.at >= horizon_start) {
            Ok((baselines, entries)) => (baselines, entries, true),
            Err(err) => {
                // Without the existing baselines every live total would be
                // re-appended as new spend. Keep counting in memory for this
                // run, but never write to a ledger we could not read.
                tracing::warn!(
                    event = "usage.cost_ledger.load",
                    outcome = "error",
                    path = %path.display(),
                    err = %err,
                    "failed to read cost ledger; cost will not be persisted this run"
                );
                (mercury_usage::CostBaselines::default(), Vec::new(), false)
            }
        };
    if !entries.is_empty()
        && event_tx
            .blocking_send(AppEvent::UsageCostRecorded { entries })
            .is_err()
    {
        return;
    }
    while let Ok(observation) = rx.recv() {
        let Some(entry) = baselines.observe(observation) else {
            continue;
        };
        if persist {
            if let Err(err) = ledger::append(&path, &entry) {
                tracing::warn!(
                    event = "usage.cost_ledger.append",
                    outcome = "error",
                    path = %path.display(),
                    err = %err,
                    "failed to append to cost ledger"
                );
            }
        }
        if event_tx
            .blocking_send(AppEvent::UsageCostRecorded {
                entries: vec![entry],
            })
            .is_err()
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mercury_usage::Provider;

    fn temp_ledger(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "mercury-cost-ledger-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |elapsed| elapsed.as_nanos())
            ))
            .join("cost-ledger.jsonl")
    }

    fn observation(total: f64) -> CostObservation {
        CostObservation {
            at: OffsetDateTime::now_utc(),
            provider: Provider::Claude,
            session_id: "sess-1".into(),
            cwd: Some("/work".into()),
            workspace: None,
            agent: None,
            total_usd: total,
        }
    }

    fn next_entries(
        rx: &mut tokio::sync::mpsc::Receiver<AppEvent>,
    ) -> Vec<mercury_usage::CostLedgerEntry> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match rx.try_recv() {
                Ok(AppEvent::UsageCostRecorded { entries }) => return entries,
                Ok(_) => continue,
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(err) => panic!("no ledger event: {err:?}"),
            }
        }
    }

    #[test]
    fn restart_restores_past_cost_and_never_double_counts() {
        let path = temp_ledger("restart");
        let horizon = OffsetDateTime::now_utc() - time::Duration::days(35);

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(16);
        let first = CostLedgerHandle::spawn(path.clone(), horizon, event_tx);
        first.observe(observation(1.5));
        assert_eq!(next_entries(&mut event_rx)[0].cost_delta_usd, 1.5);
        first.observe(observation(2.0));
        assert!((next_entries(&mut event_rx)[0].cost_delta_usd - 0.5).abs() < 1e-12);
        drop(first);

        // "Restart": a new thread over the same file replays past spend and
        // keeps the baseline, so the same cumulative total is not new spend.
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(16);
        let second = CostLedgerHandle::spawn(path.clone(), horizon, event_tx);
        let replayed = next_entries(&mut event_rx);
        let replayed_total: f64 = replayed.iter().map(|entry| entry.cost_delta_usd).sum();
        assert!((replayed_total - 2.0).abs() < 1e-12);
        second.observe(observation(2.0));
        second.observe(observation(2.25));
        let after = next_entries(&mut event_rx);
        assert_eq!(after.len(), 1);
        assert!((after[0].cost_delta_usd - 0.25).abs() < 1e-12);

        let lines = std::fs::read_to_string(&path).unwrap();
        assert_eq!(lines.lines().count(), 3);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
