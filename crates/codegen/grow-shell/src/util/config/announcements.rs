use toml::Value as TomlValue;

/// Announcement entry shared with the TUI.
pub use grow_announcements::Announcement;

/// Resolve announcements from the final effective local configuration.
///
/// The effective configuration has already applied Grow's normal configuration
/// precedence. A declared array replaces the built-in content wholesale;
/// `announcements = []` disables the feature. When the key is absent, Grow's
/// single built-in announcement is used.
pub fn resolve_announcements(root: &TomlValue) -> Vec<Announcement> {
    let Some(value) = root.get("announcements") else {
        return grow_announcements::default_announcements();
    };
    match value.clone().try_into::<Vec<Announcement>>() {
        Ok(announcements) => announcements,
        Err(error) => {
            tracing::warn!(%error, "invalid local announcements configuration; using built-in default");
            grow_announcements::default_announcements()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_config_uses_grow_default() {
        let root: TomlValue = toml::from_str("").unwrap();
        let resolved = resolve_announcements(&root);
        assert_eq!(resolved, grow_announcements::default_announcements());
        assert_eq!(resolved[0].id.as_deref(), Some("grow-default"));
    }

    #[test]
    fn configured_list_replaces_default() {
        let root: TomlValue = toml::from_str(
            r#"
                [[announcements]]
                id = "team-notice"
                title = "Notice"
                message = "Local announcement text"
                severity = "info"
                dismissible = true

                [announcements.cta]
                label = "Open docs"
                url = "https://example.com/docs"
            "#,
        )
        .unwrap();
        let resolved = resolve_announcements(&root);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id.as_deref(), Some("team-notice"));
        assert_eq!(
            resolved[0]
                .cta
                .as_ref()
                .and_then(|cta| cta.label.as_deref()),
            Some("Open docs")
        );
    }

    #[test]
    fn explicit_empty_list_disables_announcements() {
        let root: TomlValue = toml::from_str("announcements = []").unwrap();
        assert!(resolve_announcements(&root).is_empty());
    }
}
