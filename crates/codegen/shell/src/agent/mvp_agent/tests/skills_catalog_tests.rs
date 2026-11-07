use super::{
    build_minimal_agent_for_tests, make_test_handle_with_receiver, run_local_for_bridge_test,
};
use crate::session::SessionCommand;
use acp_transport::{AcpAgentHandler as _, protocol as acp};
use serde_json::json;
use std::time::Duration;

fn request(method: &str, params: serde_json::Value) -> acp::ExtRequest {
    acp::ExtRequest::new(
        method.to_owned(),
        serde_json::value::to_raw_value(&params).unwrap().into(),
    )
}

#[test]
fn plugins_list_distinguishes_acknowledged_none_from_closed_channels() {
    run_local_for_bridge_test(|| async {
        let (handle, mut commands) = make_test_handle_with_receiver("test/default", None);
        let (response, ()) = tokio::join!(handle.plugins_list(), async {
            let Some(SessionCommand::PluginsList { respond_to }) =
                tokio::time::timeout(Duration::from_secs(5), commands.recv())
                    .await
                    .expect("PluginsList command must arrive")
            else {
                panic!("expected plugin snapshot command");
            };
            assert!(respond_to.send(None).is_ok());
        });
        assert!(
            response.unwrap().is_none(),
            "acknowledged None is a successful empty snapshot"
        );

        let (response, ()) = tokio::join!(handle.plugins_list(), async {
            let Some(SessionCommand::PluginsList { respond_to }) =
                tokio::time::timeout(Duration::from_secs(5), commands.recv())
                    .await
                    .expect("PluginsList command must arrive")
            else {
                panic!("expected plugin snapshot command");
            };
            drop(respond_to);
        });
        assert!(
            response
                .unwrap_err()
                .to_string()
                .contains("reply channel closed")
        );
        drop(commands);
        assert!(
            handle
                .plugins_list()
                .await
                .unwrap_err()
                .to_string()
                .contains("command channel closed")
        );
    });
}

#[test]
fn skills_and_plugins_propagate_owner_channel_failures() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let (handle, mut commands) = make_test_handle_with_receiver("test/default", None);
        let id = handle.info.id.clone();
        agent.sessions.borrow_mut().insert(id.clone(), handle);
        for method in [
            "grow/skills/add",
            "grow/skills/remove",
            "grow/skills/reset",
            "grow/skills/config",
            "grow/skills/toggle",
            "grow/skills/list",
            "grow/plugins/list",
        ] {
            let req = request(
                method,
                json!({"sessionId": id.0, "path": "must-not-save",
                "name": "must-not-toggle", "enabled": false}),
            );
            let (result, ()) = tokio::join!(agent.ext_method(req), async {
                let Some(SessionCommand::PluginsList { respond_to }) =
                    tokio::time::timeout(Duration::from_secs(5), commands.recv())
                        .await
                        .expect("PluginsList command must arrive")
                else {
                    panic!("expected plugin snapshot command for {method}");
                };
                drop(respond_to);
            });
            let error = result.expect_err("lost owner reply must be an error before scan/mutation");
            assert!(
                format!("{error:?}").contains("reply channel closed"),
                "{method}: {error:?}"
            );
        }
        drop(commands);
        for method in ["grow/skills/reset", "grow/plugins/list"] {
            let error = agent
                .ext_method(request(method, json!({"sessionId": id.0})))
                .await
                .expect_err("closed owner must not return native-only/empty success");
            assert!(
                format!("{error:?}").contains("command channel closed"),
                "{method}: {error:?}"
            );
        }
    });
}

#[test]
fn skills_reject_child_only_owner_before_any_command() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let (mut child, mut commands) = make_test_handle_with_receiver("test/default", None);
        let id = acp::SessionId::new("skills-child-only");
        child.info.id = id.clone();
        agent
            .active_child_sessions
            .borrow_mut()
            .insert(id.clone(), child);
        for method in ["add", "remove", "reset", "config", "toggle", "list"] {
            let error = agent
                .ext_method(request(
                    &format!("grow/skills/{method}"),
                    json!({"sessionId": id.0, "path": "must-not-save",
                    "name": "must-not-toggle", "enabled": false}),
                ))
                .await
                .expect_err("only exact primary owners can supply Skills context");
            assert!(format!("{error:?}").contains("unknown primary session id"));
        }
        assert!(matches!(
            commands.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    });
}
