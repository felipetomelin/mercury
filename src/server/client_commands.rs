use std::io;
use std::sync::mpsc;

use tokio::sync::mpsc as tokio_mpsc;

use crate::api::schema::{ErrorBody, ErrorResponse, Method};

use super::client_transport::ServerEvent;

pub(crate) const MAX_ENDPOINT_COMMAND_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_ENDPOINT_BOOT_ID_BYTES: usize = 128;
pub(crate) const MAX_ENDPOINT_REQUEST_ID_BYTES: usize = 128;
const ENDPOINT_RESPONSE_CHUNK_BYTES: usize = 512 * 1024;

const CLIENT_SHELL_METHODS: &[&str] = &[
    "agent.start",
    "client_shell.surface.set",
    "command.invoke",
    "cron.add",
    "cron.edit",
    "cron.get",
    "cron.list",
    "cron.log",
    "cron.pause",
    "cron.preview",
    "cron.rm",
    "cron.run",
    "cron.runs",
    "history.resolve",
    "history.search",
    "integration.install",
    "integration.list",
    "knowledge.request",
    "layout.set_split_ratio",
    "learn.diff",
    "learn.list",
    "learn.show",
    "learn.status",
    "learn.undo",
    "pane.clear",
    "pane.close",
    "pane.copy_motion",
    "pane.copy_search",
    "pane.edit_scrollback",
    "pane.focus",
    "pane.focus_direction",
    "pane.input.set",
    "pane.link.activate",
    "pane.link.resolve",
    "pane.rename",
    "pane.resize",
    "pane.scroll",
    "pane.selection.read",
    "pane.split",
    "pane.swap",
    "pane.zoom",
    "product_announcement.dismiss",
    "release_notes.dismiss",
    "server.reload_config",
    "tab.close",
    "tab.create",
    "tab.focus",
    "tab.move",
    "tab.rename",
    "usage.refresh",
    "workspace.close",
    "workspace.create",
    "workspace.focus",
    "workspace.move",
    "workspace.move_block",
    "workspace.rename",
    "worktree.create",
    "worktree.list",
    "worktree.open",
    "worktree.remove",
];

pub(crate) fn supported_client_shell_method_names() -> &'static [&'static str] {
    CLIENT_SHELL_METHODS
}

pub(crate) fn supports_client_shell_method_name(method: &str) -> bool {
    CLIENT_SHELL_METHODS.contains(&method)
}

pub(crate) fn supports_client_shell_method(method: &Method) -> bool {
    supports_client_shell_method_name(crate::api::api_method_name(method))
}

pub(crate) fn error_response(id: String, code: &str, message: impl Into<String>) -> String {
    serde_json::to_string(&ErrorResponse {
        id,
        error: ErrorBody {
            code: code.into(),
            message: message.into(),
        },
    })
    .unwrap_or_else(|_| {
        r#"{"id":"","error":{"code":"serialization_error","message":"failed to serialize endpoint response"}}"#.into()
    })
}

pub(crate) fn success_message_with_result(
    boot_id: String,
    request_id: String,
    result: crate::api::schema::ResponseResult,
) -> crate::protocol::ServerMessage {
    let response = serde_json::to_string(&crate::api::schema::SuccessResponse {
        id: request_id.clone(),
        result,
    })
    .unwrap_or_else(|_| {
        error_response(
            request_id.clone(),
            "serialization_error",
            "failed to serialize endpoint response",
        )
    });
    crate::protocol::ServerMessage::ClientShellEndpointResponseChunk {
        boot_id,
        request_id,
        final_chunk: true,
        data: response.into_bytes(),
    }
}

pub(crate) fn error_message(
    boot_id: String,
    request_id: String,
    code: &str,
    message: impl Into<String>,
) -> crate::protocol::ServerMessage {
    let response = error_response(request_id.clone(), code, message);
    crate::protocol::ServerMessage::ClientShellEndpointResponseChunk {
        boot_id,
        request_id,
        final_chunk: true,
        data: response.into_bytes(),
    }
}

fn correlate_response_id(response: String, request_id: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&response) else {
        return response;
    };
    let Some(id) = value.get_mut("id") else {
        return response;
    };
    if id.as_str() == Some(request_id) {
        return response;
    }
    *id = serde_json::Value::String(request_id.to_owned());
    serde_json::to_string(&value).unwrap_or(response)
}

pub(crate) fn spawn_response_waiter(
    client_id: u64,
    boot_id: String,
    request_id: String,
    response_rx: mpsc::Receiver<String>,
    server_event_tx: tokio_mpsc::Sender<ServerEvent>,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name("mercury-client-endpoint-response".into())
        .spawn(move || {
            let response = response_rx.recv().unwrap_or_else(|_| {
                error_response(
                    request_id.clone(),
                    "server_unavailable",
                    "endpoint command ended without a response",
                )
            });
            let response = correlate_response_id(response, &request_id).into_bytes();
            if response.is_empty() {
                let _ = server_event_tx.blocking_send(
                    ServerEvent::ClientShellEndpointResponseChunkReady {
                        client_id,
                        boot_id,
                        request_id,
                        final_chunk: true,
                        data: Vec::new(),
                    },
                );
                return;
            }
            let chunk_count = response.len().div_ceil(ENDPOINT_RESPONSE_CHUNK_BYTES);
            for (index, chunk) in response.chunks(ENDPOINT_RESPONSE_CHUNK_BYTES).enumerate() {
                if server_event_tx
                    .blocking_send(ServerEvent::ClientShellEndpointResponseChunkReady {
                        client_id,
                        boot_id: boot_id.clone(),
                        request_id: request_id.clone(),
                        final_chunk: index + 1 == chunk_count,
                        data: chunk.to_vec(),
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use sha2::{Digest, Sha256};

    use super::*;

    fn collect_schema_refs(value: &serde_json::Value, refs: &mut BTreeSet<String>) {
        match value {
            serde_json::Value::Object(object) => {
                if let Some(reference) = object.get("$ref").and_then(serde_json::Value::as_str) {
                    if let Some(name) = reference.rsplit('/').next() {
                        refs.insert(name.to_owned());
                    }
                }
                for value in object.values() {
                    collect_schema_refs(value, refs);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    collect_schema_refs(value, refs);
                }
            }
            _ => {}
        }
    }

    fn normalized_wire_schema(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(object) => serde_json::Value::Object(
                object
                    .iter()
                    .filter(|(key, _)| {
                        !matches!(
                            key.as_str(),
                            "description" | "examples" | "readOnly" | "title" | "writeOnly"
                        )
                    })
                    .map(|(key, value)| (key.clone(), normalized_wire_schema(value)))
                    .collect(),
            ),
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.iter().map(normalized_wire_schema).collect())
            }
            _ => value.clone(),
        }
    }

    fn endpoint_method_shape_digests() -> BTreeMap<String, String> {
        let schema = serde_json::to_value(schemars::schema_for!(crate::api::schema::Request))
            .expect("request schema");
        let definitions = schema
            .get("$defs")
            .and_then(serde_json::Value::as_object)
            .expect("request definitions");
        let branches = schema
            .get("oneOf")
            .and_then(serde_json::Value::as_array)
            .expect("request method branches");
        let mut digests = BTreeMap::new();

        for method in CLIENT_SHELL_METHODS {
            let branch = branches
                .iter()
                .find(|branch| {
                    branch
                        .pointer("/properties/method/const")
                        .and_then(serde_json::Value::as_str)
                        == Some(method)
                })
                .unwrap_or_else(|| panic!("missing request schema branch for {method}"));
            let mut referenced_names = BTreeSet::new();
            collect_schema_refs(branch, &mut referenced_names);
            let mut visited_names = BTreeSet::new();
            let mut selected_definitions = serde_json::Map::new();
            while let Some(name) = referenced_names.pop_first() {
                if !visited_names.insert(name.clone()) {
                    continue;
                }
                let definition = definitions
                    .get(&name)
                    .unwrap_or_else(|| panic!("missing schema definition {name} for {method}"));
                collect_schema_refs(definition, &mut referenced_names);
                selected_definitions.insert(name, normalized_wire_schema(definition));
            }
            let shape = serde_json::json!({
                "request": normalized_wire_schema(branch),
                "definitions": selected_definitions,
            });
            let bytes = serde_json::to_vec(&shape).expect("method shape json");
            digests.insert(method.to_string(), format!("{:x}", Sha256::digest(bytes)));
        }

        digests
    }

    #[test]
    fn advertised_client_shell_method_shapes_stay_at_the_v1_contract() {
        let expected: BTreeMap<String, String> = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/endpoint-method-shapes-v1.json"
        )))
        .expect("endpoint method shape fixture");
        let mut actual = endpoint_method_shape_digests();
        // Freeze additive methods separately without rewriting the published fixture.
        for (method, digest) in [
            (
                "cron.preview",
                "49a538d76577d99c566d1ee9c2323ad2162ea50199debf757fcc64615c96ef03",
            ),
            (
                "cron.rm",
                "72a0d42d94c1cdbdbba2d1bcc97f5e85dded134974fa7bf3d08ca14e17f9ec9d",
            ),
            (
                "usage.refresh",
                "26e54fee93b05af5f0ea28b9aed51cbbc3311eb24170316e7b5fe48e257f330a",
            ),
        ] {
            assert_eq!(actual.remove(method).as_deref(), Some(digest));
        }
        assert_eq!(
            actual.remove("pane.clear").as_deref(),
            Some("0301d288ba198ddaa427dd7421c71911cccaf4ea03544531efa8b67ca21b08f6")
        );
        assert_eq!(
            actual.remove("pane.link.resolve").as_deref(),
            Some("f5e4a3e01453ae7b188f127ce951c12c20e0bebcc17cc364eeb6d1a01fd5bf81")
        );
        // cron.add/edit changed only after the requested removal of the Email
        // delivery variant from the shared CronJobDefinition schema. The five
        // Learn digests below freeze newly advertised TUI methods.
        for (method, digest) in [
            (
                "knowledge.request",
                "adb26e303a250e638ed420d1e24f101c7ba28623910b15a4c0cd11fc412080c5",
            ),
            (
                "agent.start",
                "2c70813601c631682efb857fe6286384ba1d6ddafbbb8d420ad6e548522c74a7",
            ),
            (
                "cron.add",
                "fb27902a1d75e64391544cc4aae6432122534007945d0992df52f65da719e9ee",
            ),
            (
                "cron.edit",
                "1bd5678f246e5ca787cfc93a5186f7c6dadc22821f333d0a7d189c18d7e582ab",
            ),
            (
                "cron.get",
                "6b965b8b06503693b2bd6b9feb45d5e71d75c30a8c19cd07378f6c7b95b75504",
            ),
            (
                "cron.list",
                "6cce49656f8e88e1f4b211025395f54b1c4542e0d38ec84e83079e12df564f0f",
            ),
            (
                "cron.log",
                "beddb3fbee5d38d956e49de43c243fc01180a9caae840f3328de1b51ad68a117",
            ),
            (
                "cron.pause",
                "f0c687e7de5e32b88814e70ea5d43604b44fb7c2c9c82a5c25998856fbe2d5d2",
            ),
            (
                "cron.run",
                "e58884920d270bb23728351574218098b6f18a47da0f7f1a2a7ff1e4d466374d",
            ),
            (
                "cron.runs",
                "ed8d5ace02177826e047b7d28e862aba8fc8891a13b07c8d7a7b4d2fa7099345",
            ),
            (
                "history.resolve",
                "13be52f65d43780f8a5f3427cb7757ab020990e0074bf5cfcd37adbb2226c384",
            ),
            (
                "history.search",
                "7b0b5b86a1ef328a8b9d0edd7aa3dec3dd42216456ec996f452771f972fe4077",
            ),
            (
                "learn.diff",
                "dfd1bce9eb6b446a0dd1d28ea2ff4eb7559ed637b784388c2b781a814a211f29",
            ),
            (
                "learn.list",
                "9ad98546df32f74743013cfd4441fbe8e28ea4b8374539144afc8eb28f3c3fd3",
            ),
            (
                "learn.show",
                "67be358e0f5fa5c548e9cf2446b164ffd864430cd7af58ec1cbdab1dd48006d2",
            ),
            (
                "learn.status",
                "c86d469860a0322184c9112c2ab1b06545fb22240b962a8c4e1dc793d251d2c4",
            ),
            (
                "learn.undo",
                "8e7e91ae3be426a0ca44b9b7f34c1dbacf9c63e1b7aa9c4706bfc5c0fad5383f",
            ),
        ] {
            assert_eq!(actual.remove(method).as_deref(), Some(digest), "{method}");
        }

        assert_eq!(
            actual, expected,
            "an existing endpoint method changed shape; add load-bearing behavior as a new advertised method or explicitly gate new fields"
        );
    }

    #[test]
    fn advertised_client_shell_methods_are_sorted_unique_and_in_schema() {
        assert!(CLIENT_SHELL_METHODS
            .windows(2)
            .all(|pair| pair[0] < pair[1]));

        fn collect_method_constants(value: &serde_json::Value, methods: &mut Vec<String>) {
            match value {
                serde_json::Value::Object(object) => {
                    if let Some(method) = object
                        .get("const")
                        .and_then(serde_json::Value::as_str)
                        .filter(|value| value.contains('.'))
                    {
                        methods.push(method.to_owned());
                    }
                    for value in object.values() {
                        collect_method_constants(value, methods);
                    }
                }
                serde_json::Value::Array(values) => {
                    for value in values {
                        collect_method_constants(value, methods);
                    }
                }
                _ => {}
            }
        }

        let schema = serde_json::to_value(schemars::schema_for!(crate::api::schema::Request))
            .expect("request schema");
        let mut schema_methods = Vec::new();
        collect_method_constants(&schema, &mut schema_methods);
        for method in CLIENT_SHELL_METHODS {
            assert!(
                schema_methods.iter().any(|candidate| candidate == method),
                "advertised endpoint method {method:?} is absent from the request schema"
            );
        }
    }

    #[test]
    fn cron_tab_methods_are_advertised_to_the_client_shell() {
        for method in [
            "cron.add",
            "cron.edit",
            "cron.get",
            "cron.list",
            "cron.log",
            "cron.pause",
            "cron.preview",
            "cron.rm",
            "cron.run",
            "cron.runs",
        ] {
            assert!(supports_client_shell_method_name(method), "{method}");
        }
    }

    #[test]
    fn learn_tab_methods_are_advertised_without_exposing_hook_or_date_methods() {
        for method in [
            "learn.diff",
            "learn.list",
            "learn.show",
            "learn.status",
            "learn.undo",
        ] {
            assert!(supports_client_shell_method_name(method), "{method}");
        }
        for method in [
            "learn.claude_session_end",
            "learn.claude_turn_end",
            "learn.undo_date",
        ] {
            assert!(!supports_client_shell_method_name(method), "{method}");
        }
    }

    #[test]
    fn client_shell_lane_excludes_api_front_door_and_lifecycle_methods() {
        assert!(supports_client_shell_method(
            &Method::ClientShellSurfaceSet(crate::api::schema::ClientShellSurfaceSetParams {
                active: false,
            })
        ));
        assert!(supports_client_shell_method(&Method::ServerReloadConfig(
            crate::api::schema::EmptyParams::default(),
        )));
        assert!(supports_client_shell_method(&Method::PaneLinkActivate(
            crate::api::schema::PaneLinkActivateParams {
                pane_id: "w1:p1".into(),
                viewport_row: 0,
                col: 0,
                content_revision: None,
                offset_from_bottom: None,
            },
        )));
        assert!(supports_client_shell_method(&Method::PaneLinkResolve(
            crate::api::schema::PaneLinkActivateParams {
                pane_id: "w1:p1".into(),
                viewport_row: 0,
                col: 0,
                content_revision: None,
                offset_from_bottom: None,
            },
        )));
        assert!(!supports_client_shell_method(&Method::Ping(
            crate::api::schema::PingParams::default(),
        )));
        assert!(!supports_client_shell_method(&Method::ServerStop(
            crate::api::schema::EmptyParams::default(),
        )));
    }

    #[test]
    fn endpoint_response_uses_the_client_request_id() {
        let response = serde_json::json!({
            "id": "endpoint:boot-a:7:client-shell:1",
            "result": { "type": "ok" }
        })
        .to_string();

        let correlated = correlate_response_id(response, "client-shell:1");
        let decoded: serde_json::Value = serde_json::from_str(&correlated).expect("response json");

        assert_eq!(decoded["id"], "client-shell:1");
    }

    #[test]
    fn endpoint_responses_are_chunked_without_truncation() {
        let (response_tx, response_rx) = mpsc::channel();
        let (event_tx, mut event_rx) = tokio_mpsc::channel(8);
        spawn_response_waiter(
            7,
            "boot-a".into(),
            "request-a".into(),
            response_rx,
            event_tx,
        )
        .unwrap();
        let response = "x".repeat(ENDPOINT_RESPONSE_CHUNK_BYTES + 17);
        response_tx.send(response.clone()).unwrap();

        let mut received = Vec::new();
        loop {
            let ServerEvent::ClientShellEndpointResponseChunkReady {
                client_id,
                boot_id,
                request_id,
                final_chunk,
                data,
            } = event_rx.blocking_recv().expect("response chunk")
            else {
                panic!("expected response chunk");
            };
            assert_eq!(client_id, 7);
            assert_eq!(boot_id, "boot-a");
            assert_eq!(request_id, "request-a");
            received.extend(data);
            if final_chunk {
                break;
            }
        }

        assert_eq!(received, response.as_bytes());
    }
}
