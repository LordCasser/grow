// Temporarily include inside session_notification.rs under #[cfg(test)].
use super::*;
use shell::extensions::notification::ImageRecoveryOperationState as Recovery;

#[test]
fn audit_v240_cursor_reconnect_merges_image_operation() {
    let mut view = crate::app::agent_view::test_agent_view(Some("s1"), "/tmp".into());
    apply_image_recovery_operation(
        "op1".into(),
        Some("p1".into()),
        Recovery::Started {
            message: "Preparing image recovery…".into(),
        },
        &mut view.scrollback,
    );
    view.begin_session_reload(1);
    // Live completion arrives while a cursor-found reload owns a fresh tail.
    apply_image_recovery_operation(
        "op1".into(),
        Some("p1".into()),
        Recovery::Completed {
            message: "Converted 1 image".into(),
        },
        &mut view.scrollback,
    );
    assert!(view.finalize_reload_and_maybe_adopt(1, true, Some("p1".into())));
    let rows = view
        .scrollback
        .iter_entries()
        .filter_map(|(_, entry)| match &entry.block {
            RenderBlock::ImageRecovery(row) => Some(row),
            _ => None,
        })
        .collect::<Vec<_>>();
    eprintln!("cursor rows: {:?}", rows);
    assert_eq!(
        rows.len(),
        1,
        "same operation must remain one row after cursor reconnect"
    );
    assert!(!rows[0].running);
    assert_eq!(rows[0].message, "Converted 1 image");
}

#[test]
fn audit_v240_child_routes_image_recovery() {
    let mut parent = crate::app::agent_view::test_agent_view(Some("parent"), "/tmp".into());
    let child = crate::app::agent_view::test_agent_view(Some("child"), "/tmp".into());
    parent
        .subagent_views
        .insert("child".into(), Box::new(child));
    let changed = handle_child_session_notification(
        GrowSessionUpdate::ImageRecoveryOperation {
            operation_id: "op-child".into(),
            prompt_id: Some("p-child".into()),
            state: Recovery::Started {
                message: "Preparing image recovery…".into(),
            },
        },
        "child",
        Some("child-1".into()),
        false,
        &mut parent,
    );
    let rows = parent.subagent_views["child"]
        .scrollback
        .iter_entries()
        .filter(|(_, entry)| matches!(&entry.block, RenderBlock::ImageRecovery(_)))
        .count();
    eprintln!("child changed: {changed}, recovery rows: {rows}");
    assert!(
        changed,
        "new child recovery must update its visible projection"
    );
    assert_eq!(rows, 1);
}
