use toml::Value as TomlValue;

/// How the agent handles tool execution permissions. Defined in
/// `diagnostics`; re-exported here so existing call sites continue
/// to work.
pub use ::diagnostics::enums::PermissionMode;

/// Parse a `permission_mode` canonical string to `PermissionMode`.
///
/// Valid values: `"always-approve"` → `AlwaysApprove`, `"auto"` → `Auto`,
/// `"ask"` → `Ask`.
/// Unknown strings fall back to `Ask` (safe direction).
pub fn parse_permission_mode_canonical(mode_str: &str) -> PermissionMode {
    match mode_str {
        "always-approve" => PermissionMode::AlwaysApprove,
        "auto" => PermissionMode::Auto,
        "ask" => PermissionMode::Ask,
        _ => PermissionMode::Ask,
    }
}

/// Canonical `[ui] permission_mode` string for a resolved [`PermissionMode`].
///
/// Inverse of [`parse_permission_mode_canonical`] for the real variants, so
/// `parse_permission_mode_canonical(permission_mode_canonical_str(m)) == m`.
pub fn permission_mode_canonical_str(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::AlwaysApprove => "always-approve",
        PermissionMode::Auto => "auto",
        PermissionMode::Ask => "ask",
    }
}

/// Parse the canonical `[ui] permission_mode` when explicitly set. Unknown or
/// non-string values fail closed to `Ask`; an absent key leaves remote/default
/// resolution in control.
pub fn permission_mode_from_ui_if_set(ui: &TomlValue) -> Option<PermissionMode> {
    let table = ui.as_table()?;
    let value = table.get("permission_mode")?;
    Some(
        value
            .as_str()
            .map(parse_permission_mode_canonical)
            .unwrap_or(PermissionMode::Ask),
    )
}

/// Pure resolver: effective TOML `[ui]` permission keys (if any) >
/// the supplied default `permission_mode` > `Ask`.
pub fn resolve_permission_mode(
    effective_ui: Option<&TomlValue>,
    remote_permission_mode: Option<&str>,
) -> PermissionMode {
    if let Some(ui) = effective_ui
        && let Some(mode) = permission_mode_from_ui_if_set(ui)
    {
        return mode;
    }
    if let Some(mode_str) = remote_permission_mode {
        return parse_permission_mode_canonical(mode_str);
    }
    PermissionMode::Ask
}

/// Display projection for a selected mode that did not survive the feature
/// gate: Auto shows as Ask so the UI never claims more than enforcement grants.
pub fn clamped_display_permission_mode(mode: PermissionMode) -> &'static str {
    if mode.is_always_approve() || mode.is_auto() {
        "ask"
    } else {
        permission_mode_canonical_str(mode)
    }
}

/// Displayed mode for a non-CLI resolution (effective TOML > supplied default > Ask),
/// clamped per [`clamped_display_permission_mode`].
pub fn resolved_display_permission_mode(
    effective_ui: Option<&TomlValue>,
    remote_permission_mode: Option<&str>,
) -> &'static str {
    let mode = resolve_permission_mode(effective_ui, remote_permission_mode);
    clamped_display_permission_mode(mode)
}

/// Load selected permission mode for launch (effective TOML + explicit remote).
///
/// TOML `[ui] permission_mode` wins over remote; remote only when it is absent.
/// Missing/unknown → Ask. Config load failure → Ask.
///
/// Accepts (TOML):
///   permission_mode = "always-approve"
///   permission_mode = "auto"
///   permission_mode = "ask"
pub fn load_permission_mode(remote_permission_mode: Option<&str>) -> PermissionMode {
    let root: TomlValue = match crate::config::load_effective_config() {
        Ok(r) => r,
        Err(_) => return PermissionMode::Ask,
    };
    let ui = root.as_table().and_then(|t| t.get("ui"));
    resolve_permission_mode(ui, remote_permission_mode)
}

/// Canonical permission-mode resolution for one launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveLaunchPermission {
    pub mode: PermissionMode,
}

/// Resolve the single permission mode for a launch. CLI `--permission-mode`
/// beats `[ui] permission_mode`; the Auto feature gate clamps the result to Ask
/// at this boundary when disabled.
///
/// `remote_permission_mode` is the soft-default when no TOML permission key is
/// set; pass `None` when remote settings are unavailable.
pub fn effective_permission_mode_for_launch(
    cli_permission_mode: Option<&str>,
    remote_permission_mode: Option<&str>,
) -> EffectiveLaunchPermission {
    let requested = cli_permission_mode
        .map(parse_permission_mode_canonical)
        .unwrap_or_else(|| load_permission_mode(remote_permission_mode));
    let mode = match requested {
        PermissionMode::Auto if !crate::util::config::auto_permission_mode_enabled_from_disk() => {
            PermissionMode::Ask
        }
        mode => mode,
    };
    EffectiveLaunchPermission { mode }
}

/// Whether a session should activate the **auto** permission mode: the feature
/// gate must be enabled and Auto requested. Pure so the agent's activation seam
/// is unit-testable without a live session.
pub fn auto_mode_session_active(gate_enabled: bool, requested_mode: PermissionMode) -> bool {
    gate_enabled && requested_mode.is_auto()
}

/// Load `[ui] require_plan_approval` from config.toml.
///
/// When `true`, the plan viewer always opens for explicit user approval
/// when PlanControl submits a plan, even in always-approve
/// mode. Defaults to `false`.
pub fn load_require_plan_approval() -> bool {
    let root: TomlValue = match crate::config::load_effective_config() {
        Ok(r) => r,
        Err(_) => return false,
    };
    root.as_table()
        .and_then(|t| t.get("ui"))
        .and_then(|v| v.as_table())
        .and_then(|ui| ui.get("require_plan_approval"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Synchronously load the remote agent secret from the config file.
/// Looks for [remote] section with secret field.
///
/// Example config.toml:
/// ```toml
/// [remote]
/// secret = "my-secret-token"
/// ```
pub fn load_remote_secret_sync() -> Option<String> {
    let root: TomlValue = crate::config::load_effective_config().ok()?;

    if let TomlValue::Table(table) = root
        && let Some(TomlValue::Table(remote)) = table.get("remote")
    {
        remote
            .get("secret")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_permission_mode_none_is_ask() {
        assert_eq!(resolve_permission_mode(None, None), PermissionMode::Ask);
    }

    #[test]
    fn resolve_permission_mode_remote_only() {
        assert_eq!(
            resolve_permission_mode(None, Some("auto")),
            PermissionMode::Auto,
        );
        assert_eq!(
            resolve_permission_mode(None, Some("always-approve")),
            PermissionMode::AlwaysApprove,
        );
        assert_eq!(
            resolve_permission_mode(None, Some("ask")),
            PermissionMode::Ask,
        );
    }

    #[test]
    fn resolve_permission_mode_toml_wins_over_remote() {
        let root: TomlValue = toml::from_str("[ui]\npermission_mode = \"ask\"\n").unwrap();
        assert_eq!(
            resolve_permission_mode(Some(root.get("ui").unwrap()), Some("always-approve")),
            PermissionMode::Ask,
        );
        let auto: TomlValue = toml::from_str("[ui]\npermission_mode = \"auto\"\n").unwrap();
        assert_eq!(
            resolve_permission_mode(Some(auto.get("ui").unwrap()), Some("ask")),
            PermissionMode::Auto,
        );
    }

    #[test]
    fn permission_mode_from_ui_if_set_none_when_no_keys() {
        let theme: TomlValue = toml::from_str("[ui]\ntheme = \"grownight\"\n").unwrap();
        assert_eq!(
            permission_mode_from_ui_if_set(theme.get("ui").unwrap()),
            None,
        );
        assert_eq!(
            permission_mode_from_ui_if_set(&TomlValue::String("nope".into())),
            None,
        );
    }

    #[test]
    fn resolve_permission_mode_unknown_remote_is_ask() {
        assert_eq!(
            resolve_permission_mode(None, Some("garbage")),
            PermissionMode::Ask,
        );
        assert_eq!(resolve_permission_mode(None, Some("")), PermissionMode::Ask);
    }

    #[test]
    fn parse_permission_mode_canonical_covers_all_canonicals_plus_fallback() {
        assert_eq!(
            parse_permission_mode_canonical("always-approve"),
            PermissionMode::AlwaysApprove,
        );
        assert_eq!(
            parse_permission_mode_canonical("auto"),
            PermissionMode::Auto,
        );
        assert_eq!(parse_permission_mode_canonical("ask"), PermissionMode::Ask,);
        // Unknown / corrupt → Ask (safer direction).
        assert_eq!(
            parse_permission_mode_canonical("garbage"),
            PermissionMode::Ask,
        );
        assert_eq!(parse_permission_mode_canonical(""), PermissionMode::Ask,);
        // Case sensitivity (no normalization — wire format is exact-match).
        assert_eq!(
            parse_permission_mode_canonical("Always-Approve"),
            PermissionMode::Ask,
            "wire format is case-sensitive; 'Always-Approve' is unknown",
        );
    }

    /// Canonicalization through `resolve_permission_mode` — the pure logic
    /// `load_permission_mode` delegates to. Round-trips through
    /// `permission_mode_canonical_str`.
    #[test]
    fn resolve_permission_mode_ui_precedence_and_canonicalization() {
        let cases: &[(&str, PermissionMode, &str)] = &[
            // Primary key, canonicalized.
            (
                "[ui]\npermission_mode = \"always-approve\"\n",
                PermissionMode::AlwaysApprove,
                "always-approve",
            ),
            (
                "[ui]\npermission_mode = \"auto\"\n",
                PermissionMode::Auto,
                "auto",
            ),
            (
                "[ui]\npermission_mode = \"garbage\"\n",
                PermissionMode::Ask,
                "ask",
            ),
            // No permission keys → Ask.
            ("[ui]\ntheme = \"grownight\"\n", PermissionMode::Ask, "ask"),
        ];
        for (toml_str, expected_mode, expected_canonical) in cases {
            let root: TomlValue = toml::from_str(toml_str).unwrap();
            let ui = root.get("ui").expect("test config defines [ui]");
            let mode = resolve_permission_mode(Some(ui), None);
            assert_eq!(mode, *expected_mode, "config {toml_str:?}");
            assert_eq!(
                permission_mode_canonical_str(mode),
                *expected_canonical,
                "config {toml_str:?} canonical string",
            );
        }
        // A non-table [ui] value resolves to Ask (defensive).
        assert_eq!(
            resolve_permission_mode(Some(&TomlValue::String("nope".into())), None),
            PermissionMode::Ask,
        );
    }

    /// CLI beats the remote default.
    #[test]
    fn effective_permission_mode_for_launch_cli_beats_remote() {
        assert_eq!(
            effective_permission_mode_for_launch(Some("ask"), Some("always-approve")).mode,
            PermissionMode::Ask,
        );
        assert_eq!(
            effective_permission_mode_for_launch(Some("always-approve"), Some("ask")).mode,
            PermissionMode::AlwaysApprove,
        );
    }

    /// Display clamp: modes that are not active show Ask.
    #[test]
    fn resolved_display_permission_mode_clamps_to_enforced_mode() {
        assert_eq!(
            clamped_display_permission_mode(PermissionMode::AlwaysApprove),
            "ask"
        );
        assert_eq!(clamped_display_permission_mode(PermissionMode::Auto), "ask");
        assert_eq!(clamped_display_permission_mode(PermissionMode::Ask), "ask");

        assert_eq!(resolved_display_permission_mode(None, Some("auto")), "ask");
        assert_eq!(resolved_display_permission_mode(None, None), "ask");
    }

    #[test]
    fn effective_permission_mode_for_launch_resolves_auto() {
        let _g = crate::util::config::resolve::AUTO_PERMISSION_MODE_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        unsafe { std::env::set_var("GROW_AUTO_PERMISSION_MODE", "1") };
        assert_eq!(
            effective_permission_mode_for_launch(Some("auto"), None).mode,
            PermissionMode::Auto,
        );
        unsafe { std::env::remove_var("GROW_AUTO_PERMISSION_MODE") };
    }

    /// The authoritative agent-side gate (used at the `set_auto_mode` seam):
    /// auto activates only when the feature gate is ON, auto is requested, and
    /// Gate OFF must never activate, even with an explicit Auto request.
    #[test]
    fn auto_mode_session_active_requires_gate_and_auto() {
        assert!(
            !auto_mode_session_active(false, PermissionMode::Auto),
            "gate OFF must not activate auto even when requested"
        );
        assert!(
            auto_mode_session_active(true, PermissionMode::Auto),
            "gate ON + Auto activates auto"
        );
        assert!(
            !auto_mode_session_active(true, PermissionMode::Ask),
            "Ask is inactive"
        );
        assert!(
            !auto_mode_session_active(true, PermissionMode::AlwaysApprove),
            "AlwaysApprove is not Auto"
        );
    }

    /// With the gate forced OFF (`GROW_AUTO_PERMISSION_MODE=0`), explicit
    /// `--permission-mode auto` / config auto is inert so the classifier never
    /// launches. (Compiled-in default is ON; this pins the env kill-switch.)
    #[test]
    fn effective_permission_mode_for_launch_clamps_auto_when_gate_off() {
        let _g = crate::util::config::resolve::AUTO_PERMISSION_MODE_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        unsafe { std::env::set_var("GROW_AUTO_PERMISSION_MODE", "0") };
        assert_eq!(
            effective_permission_mode_for_launch(Some("auto"), None).mode,
            PermissionMode::Ask,
        );
        unsafe { std::env::remove_var("GROW_AUTO_PERMISSION_MODE") };
    }

    // Pure tests for the policy predicate itself live next to its canonical
    // definition in the workspace permission policy module.
}
