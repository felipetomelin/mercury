//! Review a trusted session signal and apply only changes bound to the exact
//! target snapshots shown to the isolated reviewer. Signal provenance is the
//! caller's responsibility; this module never infers completion from UI state.

use std::collections::HashMap;
use std::path::Path;

use crate::policy::{Policy, PolicyError};
use crate::reviewer::{ClaudeReviewer, ReviewError, ReviewTarget};
use crate::transcript::{self, Limits, Provider, TranscriptError};
use crate::{review_snapshots, ChangeError, Changes};

pub struct SessionReview<'a> {
    pub provider: Provider,
    pub session_id: &'a str,
    pub transcript_path: &'a Path,
    /// Required exact user-turn count for a milestone, or None for end.
    pub milestone_turns: Option<usize>,
}

#[derive(Debug)]
pub enum PipelineError {
    Disabled,
    InvalidMilestone,
    Policy(PolicyError),
    Transcript(TranscriptError),
    Snapshot(ChangeError),
    Reviewer(ReviewError),
    Apply { applied: usize, error: ChangeError },
}

#[derive(Debug)]
pub struct ReviewOutcome {
    pub applied_ids: Vec<String>,
    pub observed_cost_usd: f64,
}

pub async fn review_session(
    session: SessionReview<'_>,
    policy: &Policy,
    is_enabled: impl Fn() -> bool,
    reviewer: &ClaudeReviewer,
    changes: &mut Changes,
) -> Result<ReviewOutcome, PipelineError> {
    if !is_enabled() {
        return Err(PipelineError::Disabled);
    }
    let transcript = transcript::read(
        session.transcript_path,
        session.provider,
        session.session_id,
        Limits::default(),
    )
    .map_err(PipelineError::Transcript)?;
    if let Some(turns) = session.milestone_turns {
        if turns == 0 || transcript.user_turns != Some(turns) {
            return Err(PipelineError::InvalidMilestone);
        }
    }
    let snapshots = review_snapshots(policy, is_enabled()).map_err(PipelineError::Snapshot)?;
    let targets = snapshots
        .iter()
        .map(|snapshot| ReviewTarget {
            id: snapshot.target_id.clone(),
            current_content: snapshot.content.clone(),
            max_bytes: snapshot.max_bytes,
        })
        .collect::<Vec<_>>();
    let result = reviewer
        .review(&transcript, &targets)
        .await
        .map_err(PipelineError::Reviewer)?;
    let mut applied_ids = Vec::with_capacity(result.proposals.len());
    let mut cron_after_hashes = HashMap::new();
    for proposal in result.proposals {
        if !is_enabled() {
            return Err(PipelineError::Disabled);
        }
        let target = policy
            .targets()
            .iter()
            .find(|target| target.id == proposal.target_id)
            .ok_or(PipelineError::Policy(PolicyError::UnknownTarget))?;
        let snapshot_hash = snapshots
            .iter()
            .find(|snapshot| snapshot.target_id == proposal.target_id)
            .ok_or(PipelineError::Policy(PolicyError::UnknownTarget))?
            .before_hash
            .as_str();
        let before_hash = if target.kind == crate::policy::TargetKind::CronJob {
            cron_after_hashes
                .get(&target.path)
                .map(String::as_str)
                .unwrap_or(snapshot_hash)
        } else {
            snapshot_hash
        };
        let applied = changes
            .apply(policy, is_enabled(), proposal, before_hash)
            .map_err(|error| PipelineError::Apply {
                applied: applied_ids.len(),
                error,
            })?;
        if target.kind == crate::policy::TargetKind::CronJob {
            cron_after_hashes.insert(target.path.clone(), applied.after_hash.clone());
        }
        applied_ids.push(applied.id);
    }
    Ok(ReviewOutcome {
        applied_ids,
        observed_cost_usd: result.observed_cost_usd,
    })
}
