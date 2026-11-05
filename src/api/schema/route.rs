use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RoutePreviewParams {
    pub prompt: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RoutePreviewInfo {
    pub provider: String,
    pub headroom: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_rule: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits_observed_at_unix_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub budget_excluded_providers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_limit_refresh: Option<String>,
}
