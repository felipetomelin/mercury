use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeAction {
    Status,
    Doctor,
    Index,
    Search,
    Related,
    Ask,
    JobShow,
    JobCancel,
    Capture,
    Review,
    Proposals,
    Context,
    RouteContext,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeParams {
    pub action: KnowledgeAction,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub graph: bool,
    #[serde(default)]
    pub rebuild: bool,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub title: String,
}
impl KnowledgeParams {
    pub fn new(action: KnowledgeAction) -> Self {
        Self {
            action,
            query: String::new(),
            project: None,
            kind: None,
            status: None,
            service: None,
            id: String::new(),
            mode: String::new(),
            graph: false,
            rebuild: false,
            completed: false,
            provider: String::new(),
            title: String::new(),
        }
    }
}
