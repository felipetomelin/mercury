use serde::{Deserialize, Serialize};

/// `usage.report_statusline`: the Claude Code statusline payload, forwarded
/// verbatim by `mercury statusline` (the bridge installed by
/// `mercury integration install claude`) for the pane it runs in.
///
/// `payload` is the JSON object Claude Code pipes into its `statusLine`
/// command (`session_id`, `transcript_path`, `cwd`, `model`, `workspace`,
/// `cost`, `context_window`, `rate_limits`, ...). It is the exact source for
/// context fill, session cost and, on plans that report them, account
/// windows; fields the server does not understand are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UsageReportStatuslineParams {
    pub pane_id: String,
    pub payload: serde_json::Value,
}
