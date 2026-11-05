use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};

use mercury_history::{HistoryIndex, Provider, SearchFilter};

use crate::api::schema::{
    ErrorBody, ErrorResponse, HistoryHitInfo, HistoryResumeInfo, Method, Request, ResponseResult,
    SuccessResponse,
};

const MAX_PENDING: usize = 8;
static PENDING: AtomicUsize = AtomicUsize::new(0);
static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();

pub(super) fn dispatch(request: Request, respond_to: mpsc::Sender<String>) {
    if PENDING.fetch_add(1, Ordering::AcqRel) >= MAX_PENDING {
        PENDING.fetch_sub(1, Ordering::AcqRel);
        let _ = respond_to.send(error(
            request.id,
            "history_busy",
            "history is busy; retry shortly",
        ));
        return;
    }
    tokio::task::spawn_blocking(move || {
        struct PendingGuard;
        impl Drop for PendingGuard {
            fn drop(&mut self) {
                PENDING.fetch_sub(1, Ordering::AcqRel);
            }
        }
        let _pending = PendingGuard;
        let response = match SERIAL.get_or_init(|| Mutex::new(())).lock() {
            Ok(_guard) => handle(request),
            Err(_) => error(
                request.id,
                "history_unavailable",
                "history index is unavailable",
            ),
        };
        let _ = respond_to.send(response);
    });
}

fn provider(value: &str) -> Option<Provider> {
    match value {
        "claude" => Some(Provider::Claude),
        "codex" => Some(Provider::Codex),
        _ => None,
    }
}

fn validate_date(value: &str) -> bool {
    if value.len() != 10
        || !value.bytes().enumerate().all(|(index, byte)| {
            if index == 4 || index == 7 {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
    {
        return false;
    }
    let year: u32 = value[0..4].parse().unwrap_or(0);
    let month: u32 = value[5..7].parse().unwrap_or(0);
    let day: u32 = value[8..10].parse().unwrap_or(0);
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    year > 0 && day > 0 && day <= days
}

fn handle(request: Request) -> String {
    let id = request.id;
    let method = request.method;
    if let Method::HistorySearch(params) = &method {
        if params.query.len() > 512
            || params.query.chars().any(char::is_control)
            || params.limit == 0
            || params.limit > 100
            || params
                .provider
                .as_deref()
                .is_some_and(|value| provider(value).is_none())
            || params
                .cwd
                .as_deref()
                .is_some_and(|value| !Path::new(value).is_absolute() || value.len() > 4096)
            || params
                .from
                .as_deref()
                .is_some_and(|value| !validate_date(value))
            || params
                .until
                .as_deref()
                .is_some_and(|value| !validate_date(value))
            || params
                .from
                .as_ref()
                .zip(params.until.as_ref())
                .is_some_and(|(from, until)| from > until)
        {
            return error(
                id,
                "invalid_history_search",
                "invalid history search filters",
            );
        }
    }
    if let Method::HistoryResolve(params) = &method {
        if provider(&params.provider).is_none()
            || !mercury_usage::locate::is_safe_session_id(&params.session_id)
        {
            return error(
                id,
                "invalid_history_target",
                "invalid history session target",
            );
        }
    }
    let db_path = crate::session::data_dir()
        .join("history")
        .join("index.sqlite");
    let claude_home = mercury_usage::locate::default_claude_home();
    let codex_home = mercury_usage::locate::default_codex_home();
    let Some((claude_home, codex_home)) = claude_home.zip(codex_home) else {
        return error(
            id,
            "history_unavailable",
            "agent history homes are unavailable",
        );
    };
    let mut index = match HistoryIndex::open(&db_path) {
        Ok(index) => index,
        Err(_) => return error(id, "history_unavailable", "history index is unavailable"),
    };
    if index.sync(&claude_home, &codex_home).is_err() {
        return error(id, "history_unavailable", "history scan failed");
    }
    let result = match method {
        Method::HistorySearch(params) => {
            let filter = SearchFilter {
                query: &params.query,
                provider: params.provider.as_deref().and_then(provider),
                cwd: params.cwd.as_deref(),
                from: params.from.as_deref(),
                until: params.until.as_deref(),
                limit: params.limit,
            };
            match index.search(filter) {
                Ok(hits) => ResponseResult::HistorySearch {
                    hits: hits
                        .into_iter()
                        .map(|hit| HistoryHitInfo {
                            provider: hit.provider.as_str().to_owned(),
                            session_id: hit.session_id,
                            cwd: hit.cwd,
                            timestamp: hit.timestamp,
                            snippet: hit.snippet,
                        })
                        .collect(),
                },
                Err(_) => return error(id, "history_unavailable", "history search failed"),
            }
        }
        Method::HistoryResolve(params) => {
            let provider = provider(&params.provider).expect("validated above");
            match index.resolve_resume(provider, &params.session_id) {
                Ok(target) => ResponseResult::HistoryResolve {
                    target: target.map(|target| HistoryResumeInfo {
                        provider: target.provider.as_str().to_owned(),
                        session_id: target.session_id,
                        cwd: target.cwd.to_string_lossy().into_owned(),
                    }),
                },
                Err(_) => return error(id, "history_unavailable", "history lookup failed"),
            }
        }
        _ => return error(id, "invalid_method", "invalid history method"),
    };
    serde_json::to_string(&SuccessResponse { id, result }).unwrap_or_else(|_| "{}".into())
}

fn error(id: String, code: &str, message: &str) -> String {
    serde_json::to_string(&ErrorResponse {
        id,
        error: ErrorBody {
            code: code.into(),
            message: message.into(),
        },
    })
    .unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::{HistoryResolveParams, HistorySearchParams};

    #[test]
    fn validates_calendar_dates_including_leap_years() {
        for date in ["2024-02-29", "2000-02-29", "2025-12-31"] {
            assert!(validate_date(date), "{date}");
        }
        for date in [
            "0000-01-01",
            "1900-02-29",
            "2025-02-29",
            "2025-04-31",
            "2025-13-01",
            "2025-00-01",
            "2025-01-00",
            "2025-1-01",
        ] {
            assert!(!validate_date(date), "{date}");
        }
    }

    #[test]
    fn rejects_reversed_date_range_before_touching_agent_homes() {
        let response = handle(Request {
            id: "reversed".into(),
            method: Method::HistorySearch(HistorySearchParams {
                query: String::new(),
                provider: None,
                cwd: None,
                from: Some("2025-02-02".into()),
                until: Some("2025-02-01".into()),
                limit: 10,
            }),
        });
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&response).unwrap()["error"]["code"],
            "invalid_history_search"
        );
    }

    #[test]
    fn rejects_invalid_filters_without_touching_agent_homes() {
        let response = handle(Request {
            id: "invalid".into(),
            method: Method::HistorySearch(HistorySearchParams {
                query: "hello".into(),
                provider: Some("other".into()),
                cwd: None,
                from: None,
                until: None,
                limit: 10,
            }),
        });
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&response).unwrap()["error"]["code"],
            "invalid_history_search"
        );
        let response = handle(Request {
            id: "unsafe".into(),
            method: Method::HistoryResolve(HistoryResolveParams {
                provider: "codex".into(),
                session_id: "../escape".into(),
            }),
        });
        // Invalid IDs are rejected before opening or scanning the index.
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&response).unwrap()["error"]["code"],
            "invalid_history_target"
        );
    }
}
