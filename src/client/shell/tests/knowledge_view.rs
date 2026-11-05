use super::*;
use crate::api::schema::{KnowledgeAction, Method, ResponseResult};
#[test]
fn knowledge_tab_uses_server_api_and_reads_sources_without_starting_agents() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.set_endpoint_methods(Some(
        crate::server::client_commands::supported_client_shell_method_names()
            .iter()
            .map(|s| s.to_string())
            .collect(),
    ));
    let mut out = ClientShellInput::default();
    state.toggle_knowledge_view(&mut out);
    assert_eq!(state.view, ClientShellView::Knowledge);
    let request = out
        .actions
        .iter()
        .find_map(|a| {
            if let ClientShellAction::Endpoint { request, .. } = a {
                Some(request)
            } else {
                None
            }
        })
        .unwrap();
    assert!(matches!(request.method,Method::Knowledge(ref p) if p.action==KnowledgeAction::Status));
    let id = request.id.clone();
    state.handle_endpoint_result(
        "boot-1",
        &id,
        Ok(ResponseResult::Knowledge {
            data: serde_json::json!({"enabled":true}),
        }),
    );
    assert!(!state.knowledge_view.loading);
    state.compose(110, 24).unwrap();
    state.knowledge_view.data = serde_json::json!({"revision":"r1","hits":[{"path":"A.md","status":"validated","text":"trusted-source"}]});
    state.knowledge_view.detail = true;
    state.compose(80, 12).unwrap();
    let mut out = ClientShellInput::default();
    state.toggle_knowledge_view(&mut out);
    assert_eq!(state.view, ClientShellView::Terminal);
    assert!(out.actions.is_empty());
}
