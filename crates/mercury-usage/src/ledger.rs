//! Append-only cost ledger (`cost-ledger.jsonl`, D9 in
//! `docs/dev/usage-ui-design.md`).
//!
//! Tokens are recomputable from transcripts/rollouts at any time; cost is not
//! (it only ever arrives live, e.g. from Claude Code's statusline
//! `cost.total_cost_usd`). Every positive change of a session's reported total
//! is appended here as one JSON line so USD budgets covering past windows
//! survive a Mercury restart.
//!
//! Line shape (one object per line, unknown fields ignored on read):
//! `{"at", "provider", "session_id", "cwd", "cost_delta_usd"}` plus the
//! optional `session_total_usd` (the reported cumulative total right after
//! this delta, so baselines are reconstructed exactly even across counter
//! resets) and `workspace`/`agent` attribution for scoped budgets.
//!
//! Concurrency: each entry is written with a single `write_all` of a complete
//! line to a file opened with `O_APPEND`, so concurrent appenders (several
//! Mercury servers) never interleave inside a line on a local filesystem. A
//! torn or foreign line is skipped on read, never fatal.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::model::Provider;

/// Changes smaller than this (USD) are float noise, not spend.
const COST_EPSILON_USD: f64 = 1e-9;

/// `$XDG_DATA_HOME/mercury/usage/cost-ledger.jsonl`, else
/// `~/.local/share/mercury/usage/cost-ledger.jsonl`.
pub fn default_cost_ledger_path() -> Option<PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local").join("share"))
        })?;
    Some(
        data_home
            .join("mercury")
            .join("usage")
            .join("cost-ledger.jsonl"),
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct CostLedgerEntry {
    pub at: OffsetDateTime,
    pub provider: Provider,
    pub session_id: String,
    pub cwd: Option<String>,
    pub workspace: Option<String>,
    pub agent: Option<String>,
    pub cost_delta_usd: f64,
    /// Reported cumulative session total right after this delta.
    pub session_total_usd: Option<f64>,
}

#[derive(Serialize, Deserialize)]
struct LedgerLine {
    at: String,
    provider: Provider,
    session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    cost_delta_usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session_total_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
}

impl CostLedgerEntry {
    /// One JSON object followed by `\n`.
    pub fn to_line(&self) -> String {
        let at = self
            .at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| self.at.unix_timestamp().to_string());
        let line = LedgerLine {
            at,
            provider: self.provider,
            session_id: self.session_id.clone(),
            cwd: self.cwd.clone(),
            cost_delta_usd: self.cost_delta_usd,
            session_total_usd: self.session_total_usd,
            workspace: self.workspace.clone(),
            agent: self.agent.clone(),
        };
        let mut text = serde_json::to_string(&line).expect("ledger lines are serializable");
        text.push('\n');
        text
    }

    /// `None` for torn, foreign or nonsensical lines (they are skipped).
    pub fn parse_line(line: &str) -> Option<Self> {
        let parsed: LedgerLine = serde_json::from_str(line.trim()).ok()?;
        let at = crate::parse_timestamp(&parsed.at)?;
        if !parsed.cost_delta_usd.is_finite() || parsed.cost_delta_usd <= 0.0 {
            return None;
        }
        if parsed.session_id.is_empty() {
            return None;
        }
        Some(Self {
            at,
            provider: parsed.provider,
            session_id: parsed.session_id,
            cwd: parsed.cwd,
            workspace: parsed.workspace,
            agent: parsed.agent,
            cost_delta_usd: parsed.cost_delta_usd,
            session_total_usd: parsed.session_total_usd.filter(|total| total.is_finite()),
        })
    }
}

/// A live cumulative cost report for one session.
#[derive(Debug, Clone, PartialEq)]
pub struct CostObservation {
    pub at: OffsetDateTime,
    pub provider: Provider,
    pub session_id: String,
    pub cwd: Option<String>,
    pub workspace: Option<String>,
    pub agent: Option<String>,
    pub total_usd: f64,
}

/// Last known cumulative total per session, used to turn cumulative reports
/// into deltas without double counting across restarts.
#[derive(Debug, Clone, Default)]
pub struct CostBaselines {
    totals: HashMap<(Provider, String), f64>,
}

impl CostBaselines {
    /// Rebuilds the baseline from a ledger entry read at startup.
    pub fn absorb(&mut self, entry: &CostLedgerEntry) {
        let key = (entry.provider, entry.session_id.clone());
        let total = match entry.session_total_usd {
            Some(total) => total,
            None => self.totals.get(&key).copied().unwrap_or(0.0) + entry.cost_delta_usd,
        };
        self.totals.insert(key, total);
    }

    pub fn total(&self, provider: Provider, session_id: &str) -> Option<f64> {
        self.totals.get(&(provider, session_id.to_owned())).copied()
    }

    /// Returns the entry to append, if the report carries new spend.
    ///
    /// - first report of a session: its whole total is new spend (attributed
    ///   to now — the ledger cannot know when it happened);
    /// - total grew: the difference;
    /// - total shrank (the agent restarted its counter, e.g. a resumed
    ///   process): the new total is spend since the reset, and becomes the
    ///   new baseline.
    pub fn observe(&mut self, observation: CostObservation) -> Option<CostLedgerEntry> {
        let total = observation.total_usd;
        if !total.is_finite() || total < 0.0 {
            return None;
        }
        let key = (observation.provider, observation.session_id.clone());
        let delta = match self.totals.get(&key).copied() {
            None => total,
            Some(previous) if total > previous + COST_EPSILON_USD => total - previous,
            Some(previous) if total < previous - COST_EPSILON_USD => total,
            Some(_) => return None,
        };
        self.totals.insert(key, total);
        if delta <= COST_EPSILON_USD {
            return None;
        }
        Some(CostLedgerEntry {
            at: observation.at,
            provider: observation.provider,
            session_id: observation.session_id,
            cwd: observation.cwd,
            workspace: observation.workspace,
            agent: observation.agent,
            cost_delta_usd: delta,
            session_total_usd: Some(total),
        })
    }
}

/// Reads the whole ledger: baselines from every line, entries only when
/// `keep(entry)` (callers bound memory to the budget horizon). A missing file
/// is an empty ledger.
pub fn load(
    path: &Path,
    mut keep: impl FnMut(&CostLedgerEntry) -> bool,
) -> io::Result<(CostBaselines, Vec<CostLedgerEntry>)> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Ok((CostBaselines::default(), Vec::new()))
        }
        Err(err) => return Err(err),
    };
    let mut baselines = CostBaselines::default();
    let mut entries = Vec::new();
    for line in io::BufReader::new(file).split(b'\n') {
        let line = line?;
        let Ok(text) = std::str::from_utf8(&line) else {
            continue;
        };
        let Some(entry) = CostLedgerEntry::parse_line(text) else {
            continue;
        };
        baselines.absorb(&entry);
        if keep(&entry) {
            entries.push(entry);
        }
    }
    Ok((baselines, entries))
}

/// Appends one entry (creating the file and its directory if needed; the file
/// is private to the user on Unix since it records project paths).
pub fn append(path: &Path, entry: &CostLedgerEntry) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    // One write of one complete line: O_APPEND keeps concurrent appenders
    // from interleaving inside it.
    file.write_all(entry.to_line().as_bytes())?;
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> OffsetDateTime {
        crate::parse_timestamp(text).unwrap()
    }

    fn observation(session: &str, total: f64, when: &str) -> CostObservation {
        CostObservation {
            at: at(when),
            provider: Provider::Claude,
            session_id: session.into(),
            cwd: Some("/work/app".into()),
            workspace: Some("app".into()),
            agent: None,
            total_usd: total,
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "mercury-usage-ledger-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |elapsed| elapsed.as_nanos())
            ))
            .join("usage")
            .join("cost-ledger.jsonl")
    }

    #[test]
    fn cumulative_reports_become_deltas() {
        let mut baselines = CostBaselines::default();
        let first = baselines
            .observe(observation("s1", 1.25, "2026-10-05T10:00:00Z"))
            .unwrap();
        assert_eq!(first.cost_delta_usd, 1.25);
        assert_eq!(first.session_total_usd, Some(1.25));
        assert!(baselines
            .observe(observation("s1", 1.25, "2026-10-05T10:00:01Z"))
            .is_none());
        let second = baselines
            .observe(observation("s1", 2.0, "2026-10-05T10:01:00Z"))
            .unwrap();
        assert!((second.cost_delta_usd - 0.75).abs() < 1e-12);
        // Another session has its own baseline.
        let other = baselines
            .observe(observation("s2", 0.5, "2026-10-05T10:02:00Z"))
            .unwrap();
        assert_eq!(other.cost_delta_usd, 0.5);
    }

    #[test]
    fn counter_reset_counts_spend_since_the_reset() {
        let mut baselines = CostBaselines::default();
        baselines.observe(observation("s1", 3.0, "2026-10-05T10:00:00Z"));
        let reset = baselines
            .observe(observation("s1", 0.4, "2026-10-05T11:00:00Z"))
            .unwrap();
        assert_eq!(reset.cost_delta_usd, 0.4);
        let after = baselines
            .observe(observation("s1", 0.9, "2026-10-05T11:05:00Z"))
            .unwrap();
        assert!((after.cost_delta_usd - 0.5).abs() < 1e-12);
    }

    #[test]
    fn rejects_nonsense_totals() {
        let mut baselines = CostBaselines::default();
        assert!(baselines
            .observe(observation("s1", f64::NAN, "2026-10-05T10:00:00Z"))
            .is_none());
        assert!(baselines
            .observe(observation("s1", -1.0, "2026-10-05T10:00:00Z"))
            .is_none());
        assert!(baselines
            .observe(observation("s1", 0.0, "2026-10-05T10:00:00Z"))
            .is_none());
    }

    #[test]
    fn append_then_load_round_trips_and_restores_baselines() {
        let path = temp_path("roundtrip");
        let mut baselines = CostBaselines::default();
        for (total, when) in [
            (1.0, "2026-09-01T10:00:00Z"),
            (3.0, "2026-09-01T11:00:00Z"),
            (0.5, "2026-10-05T09:00:00Z"), // counter reset
        ] {
            let entry = baselines
                .observe(observation("s1", total, when))
                .expect("new spend");
            append(&path, &entry).unwrap();
        }
        // A torn line from a crashed writer and a foreign line are skipped.
        {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            file.write_all(b"{\"at\":\"2026-10-05T09:\n").unwrap();
            file.write_all(b"not json at all\n").unwrap();
        }

        let horizon = at("2026-10-01T00:00:00Z");
        let (restored, entries) = load(&path, |entry| entry.at >= horizon).unwrap();
        assert_eq!(entries.len(), 1, "only the in-horizon entry is kept");
        assert_eq!(entries[0].cost_delta_usd, 0.5);
        assert_eq!(entries[0].cwd.as_deref(), Some("/work/app"));
        assert_eq!(entries[0].workspace.as_deref(), Some("app"));
        // The baseline is the last reported total (0.5 after the reset), not
        // the sum of deltas (1 + 2 + 0.5), so a restart never double counts.
        assert_eq!(restored.total(Provider::Claude, "s1"), Some(0.5));
        let mut restored = restored;
        assert!(restored
            .observe(observation("s1", 0.5, "2026-10-05T09:30:00Z"))
            .is_none());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).ok();
    }

    #[test]
    fn missing_ledger_is_empty() {
        let (baselines, entries) = load(&temp_path("missing"), |_| true).unwrap();
        assert!(entries.is_empty());
        assert_eq!(baselines.total(Provider::Claude, "s"), None);
    }

    #[test]
    fn line_has_the_documented_fields() {
        let entry = CostLedgerEntry {
            at: at("2026-10-05T15:00:00Z"),
            provider: Provider::Claude,
            session_id: "s1".into(),
            cwd: Some("/work".into()),
            workspace: None,
            agent: None,
            cost_delta_usd: 0.25,
            session_total_usd: Some(1.0),
        };
        let line = entry.to_line();
        assert!(line.ends_with('\n'));
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["at"], "2026-10-05T15:00:00Z");
        assert_eq!(value["provider"], "claude");
        assert_eq!(value["session_id"], "s1");
        assert_eq!(value["cwd"], "/work");
        assert_eq!(value["cost_delta_usd"], 0.25);
        assert!(value.get("workspace").is_none());
        assert_eq!(CostLedgerEntry::parse_line(&line), Some(entry));
    }
}
