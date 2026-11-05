//! `usage.*` API methods (Fase 2, D10 in `docs/dev/usage-ui-design.md`).

use crate::api::schema::{ResponseResult, UsageReportStatuslineParams};
use crate::app::App;

use super::responses::{encode_error, encode_success};

impl App {
    /// `usage.report_statusline`: exact context/cost/limits for one pane,
    /// forwarded by the `mercury statusline` bridge. The payload is parsed and
    /// validated here; everything else (overlay onto the pane's usage, cost
    /// ledger, provider limits) lives in `src/app/usage.rs`.
    pub(super) fn handle_usage_report_statusline(
        &mut self,
        id: String,
        params: UsageReportStatuslineParams,
    ) -> String {
        let Some((ws_idx, pane_id)) = self.parse_pane_id(&params.pane_id) else {
            return encode_error(
                id,
                "pane_not_found",
                format!("pane {} not found", params.pane_id),
            );
        };
        let Some(terminal_id) = self
            .state
            .workspaces
            .get(ws_idx)
            .and_then(|workspace| workspace.pane_state(pane_id))
            .map(|pane| pane.attached_terminal_id.clone())
        else {
            return encode_error(
                id,
                "pane_not_found",
                format!("pane {} not found", params.pane_id),
            );
        };
        let snapshot = match mercury_usage::StatuslineSnapshot::from_value(
            &params.payload,
            time::OffsetDateTime::now_utc(),
        ) {
            Ok(snapshot) => snapshot,
            Err(err) => return encode_error(id, "invalid_statusline", err.to_string()),
        };
        // The session id ends up in the cost ledger and is matched against
        // hook-reported ids; refuse anything that is not a plain id.
        if !mercury_usage::locate::is_safe_session_id(&snapshot.session_id) {
            return encode_error(
                id,
                "invalid_statusline",
                "statusline session_id is not a valid session id",
            );
        }
        if self.apply_statusline_report(&terminal_id, snapshot) {
            self.render_dirty.request_generic();
            self.render_notify.notify_one();
        }
        encode_success(id, ResponseResult::Ok {})
    }
}
