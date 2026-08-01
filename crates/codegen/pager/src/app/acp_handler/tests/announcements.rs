use super::*;

#[test]
fn announcements_update_replaces_local_snapshot() {
    let mut app = make_app_with_agent("sess-ann");
    app.active_announcements = vec![critical_announcement("old")];

    let changed = handle_ext_notification(
        &announcements_update_notif(&[critical_announcement("new")]),
        &mut app,
    );

    assert!(changed);
    assert_eq!(app.active_announcements, vec![critical_announcement("new")]);
    assert_eq!(app.announcement, Some(critical_announcement("new")));
}

#[test]
fn announcements_update_prunes_hidden_state_for_removed_items() {
    let mut app = make_app_with_agent("sess-ann");
    app.hidden_announcement_ids = ["old".to_string(), "kept".to_string()]
        .into_iter()
        .collect();

    let changed = handle_ext_notification(
        &announcements_update_notif(&[critical_announcement("kept")]),
        &mut app,
    );

    assert!(changed);
    assert_eq!(
        app.hidden_announcement_ids,
        ["kept".to_string()].into_iter().collect()
    );
    assert!(app.pending_effects.iter().any(
        |effect| matches!(effect, Effect::PersistAnnouncementsHidden { hidden_ids } if hidden_ids.contains("kept") && !hidden_ids.contains("old"))
    ));
}
