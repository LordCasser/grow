use super::*;

#[test]
fn announcements_update_stale_gen_is_ignored() {
    let mut app = make_app_with_agent("sess-ann");
    app.announcements_last_gen = 5;
    app.active_announcements = vec![critical_announcement("local")];

    let changed = handle_ext_notification(
        &announcements_update_notif(5, &[critical_announcement("vendor-promo")]),
        &mut app,
    );

    assert!(!changed);
    assert_eq!(app.announcements_last_gen, 5);
    assert_eq!(app.active_announcements, vec![critical_announcement("local")]);
}

#[test]
fn announcements_update_does_not_surface_vendor_promotions() {
    let mut app = make_app_with_agent("sess-ann");
    app.active_announcements = vec![critical_announcement("local")];
    app.hidden_announcement_ids = ["local".to_string()].into_iter().collect();

    let changed = handle_ext_notification(
        &announcements_update_notif(1, &[critical_announcement("grok-4-5-launch")]),
        &mut app,
    );

    assert!(changed);
    assert_eq!(app.announcements_last_gen, 1);
    assert_eq!(app.active_announcements, vec![critical_announcement("local")]);
    assert!(app.hidden_announcement_ids.contains("local"));
    assert!(
        !app.pending_effects
            .iter()
            .any(|effect| matches!(effect, Effect::PersistAnnouncementsHidden { .. }))
    );
}
