use crate::agent::config::ContextualHints;
use toml::Value as TomlValue;

/// Persisted worktree preference for `/new` and `/fork` (`[hints]` in config.toml).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorktreeHintMode {
    /// Always show the popup.
    Ask,
    /// Always create a worktree, skip the popup.
    Always,
    /// Never create a worktree, skip the popup.
    #[default]
    Never,
}

impl WorktreeHintMode {
    pub fn from_config_str(s: &str) -> Self {
        match s {
            "always" => Self::Always,
            "never" => Self::Never,
            "ask" => Self::Ask,
            other => {
                tracing::debug!(
                    value = other,
                    "unrecognised worktree hint mode, defaulting to never"
                );
                Self::Never
            }
        }
    }

    pub fn as_config_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Always => "always",
            Self::Never => "never",
        }
    }

    /// Returns `(new_session_worktree_mode, fork_worktree_mode)`.
    ///
    /// - `/new`: `new_session_worktree_mode`, else `Never`.
    /// - `/fork`: `fork_worktree_mode`, else `Ask`.
    pub fn resolve_pair(hints: Option<&TomlValue>) -> (Self, Self) {
        let get_str = |key: &str| -> Option<Self> {
            hints
                .and_then(|h| h.get(key))
                .and_then(|v| v.as_str())
                .map(Self::from_config_str)
        };
        let new_session = get_str("new_session_worktree_mode").unwrap_or(Self::Never);
        let fork = get_str("fork_worktree_mode").unwrap_or(Self::Ask);
        (new_session, fork)
    }
}

/// Resolved `[hints]` UI opt-outs (TUI "don't ask again" and related).
///
/// Read from the effective global `$GROW_HOME/config.toml`. Project config does
/// not contribute UI hints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHints {
    pub project_picker_disabled: bool,
    pub new_session_worktree_mode: WorktreeHintMode,
    pub fork_worktree_mode: WorktreeHintMode,
}

impl Default for ResolvedHints {
    fn default() -> Self {
        Self {
            project_picker_disabled: false,
            new_session_worktree_mode: WorktreeHintMode::Never,
            fork_worktree_mode: WorktreeHintMode::Ask,
        }
    }
}

impl ResolvedHints {
    fn from_hints_table(hints: Option<&TomlValue>) -> Self {
        let hint_bool = |key: &str| -> bool {
            hints
                .and_then(|h| h.get(key))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        };
        let (new_session, fork) = WorktreeHintMode::resolve_pair(hints);
        Self {
            project_picker_disabled: hint_bool("project_picker_disabled"),
            new_session_worktree_mode: new_session,
            fork_worktree_mode: fork,
        }
    }
}

/// Resolved per-tip contextual-hint gates (one bool per tip). Defaults all on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedContextualHints {
    pub undo: bool,
    pub plan_mode: bool,
    pub image_input: bool,
    pub send_now: bool,
    pub small_screen: bool,
    pub word_select: bool,
    pub ssh_wrap: bool,
}

impl Default for ResolvedContextualHints {
    fn default() -> Self {
        Self {
            undo: true,
            plan_mode: true,
            image_input: true,
            send_now: true,
            small_screen: true,
            word_select: true,
            ssh_wrap: true,
        }
    }
}

/// Resolve the per-tip contextual-hint gates. Per tip the precedence is:
/// env master `GROW_CONTEXTUAL_HINTS` (all-on/off) > local config
/// `[ui.contextual_hints].X` > default ON.
pub fn resolve_contextual_hints(ui: &ContextualHints) -> ResolvedContextualHints {
    use crate::agent::config::BoolFlag;
    let resolve_tip = |user: Option<bool>, feature_flag: Option<bool>| -> bool {
        BoolFlag::env("GROW_CONTEXTUAL_HINTS")
            .config(user)
            .feature_flag(feature_flag)
            .default(true)
            .resolve()
            .value
    };
    ResolvedContextualHints {
        undo: resolve_tip(ui.undo, None),
        plan_mode: resolve_tip(ui.plan_mode, None),
        image_input: resolve_tip(ui.image_input, None),
        send_now: resolve_tip(ui.send_now, None),
        small_screen: resolve_tip(ui.small_screen, None),
        word_select: resolve_tip(ui.word_select, None),
        ssh_wrap: resolve_tip(ui.ssh_wrap, None),
    }
}

/// Resolve `[hints]` from an effective local configuration.
pub fn resolve_hints(effective_config: Option<&TomlValue>) -> ResolvedHints {
    ResolvedHints::from_hints_table(effective_config.and_then(|root| root.get("hints")))
}

/// Load config from disk and resolve `[hints]`.
pub fn resolve_hints_from_disk() -> ResolvedHints {
    let effective = crate::config::load_effective_config().ok();
    resolve_hints(effective.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_hints_reads_local_config_when_effective_is_available() {
        let local: TomlValue = toml::from_str("[hints]\nproject_picker_disabled = true\n").unwrap();
        let resolved = resolve_hints(Some(&local));
        assert!(resolved.project_picker_disabled);
        assert_eq!(resolved.new_session_worktree_mode, WorktreeHintMode::Never);
        assert_eq!(resolved.fork_worktree_mode, WorktreeHintMode::Ask);
    }

    #[test]
    fn resolve_hints_uses_effective_root_when_provided() {
        let effective: TomlValue =
            toml::from_str("[hints]\nproject_picker_disabled = true\n").unwrap();
        let resolved = resolve_hints(Some(&effective));
        assert!(resolved.project_picker_disabled);
    }

    const ENV_CONTEXTUAL_HINTS: &str = "GROW_CONTEXTUAL_HINTS";

    // `GROW_CONTEXTUAL_HINTS` is process-global; serialize the tests reading it
    // and force it unset so a developer's shell value can't make them flaky.
    static CONTEXTUAL_HINTS_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn contextual_hints_guard() -> std::sync::MutexGuard<'static, ()> {
        let g = CONTEXTUAL_HINTS_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        unsafe { std::env::remove_var(ENV_CONTEXTUAL_HINTS) };
        g
    }

    #[test]
    fn contextual_hints_default_on_when_absent() {
        let _g = contextual_hints_guard();
        let resolved = resolve_contextual_hints(&ContextualHints::default());
        assert!(resolved.undo, "undo defaults ON");
        assert!(resolved.plan_mode, "plan_mode defaults ON");
        assert!(resolved.image_input, "image_input defaults ON");
        assert!(resolved.send_now, "send_now defaults ON");
        assert!(resolved.small_screen, "small_screen defaults ON");
        assert!(resolved.word_select, "word_select defaults ON");
        assert!(resolved.ssh_wrap, "ssh_wrap defaults ON");
    }

    #[test]
    fn contextual_hints_config_opts_out_per_tip() {
        let _g = contextual_hints_guard();
        // User disables only the undo tip; the others stay on.
        let ui = ContextualHints {
            undo: Some(false),
            ..ContextualHints::default()
        };
        let resolved = resolve_contextual_hints(&ui);
        assert!(!resolved.undo);
        assert!(resolved.plan_mode);
        assert!(resolved.image_input);
        assert!(resolved.send_now);
        assert!(resolved.small_screen);
        assert!(resolved.word_select);
        assert!(resolved.ssh_wrap);
    }

    #[test]
    fn contextual_hints_env_master_forces_all_on() {
        let _g = contextual_hints_guard();
        unsafe { std::env::set_var(ENV_CONTEXTUAL_HINTS, "1") };
        // Local config disables every tip; the env master forces all on.
        let ui = ContextualHints {
            undo: Some(false),
            plan_mode: Some(false),
            image_input: Some(false),
            send_now: Some(false),
            small_screen: Some(false),
            word_select: Some(false),
            ssh_wrap: Some(false),
        };
        let resolved = resolve_contextual_hints(&ui);
        assert!(
            resolved.undo
                && resolved.plan_mode
                && resolved.image_input
                && resolved.send_now
                && resolved.small_screen
                && resolved.word_select
                && resolved.ssh_wrap
        );
        unsafe { std::env::remove_var(ENV_CONTEXTUAL_HINTS) };
    }

    #[test]
    fn contextual_hints_env_master_zero_forces_all_off() {
        let _g = contextual_hints_guard();
        unsafe { std::env::set_var(ENV_CONTEXTUAL_HINTS, "0") };
        // Local config enables every tip; the env master forces all off.
        let ui = ContextualHints {
            undo: Some(true),
            plan_mode: Some(true),
            image_input: Some(true),
            send_now: Some(true),
            small_screen: Some(true),
            word_select: Some(true),
            ssh_wrap: Some(true),
        };
        let resolved = resolve_contextual_hints(&ui);
        assert!(
            !resolved.undo
                && !resolved.plan_mode
                && !resolved.image_input
                && !resolved.send_now
                && !resolved.small_screen
                && !resolved.word_select
                && !resolved.ssh_wrap
        );
        unsafe { std::env::remove_var(ENV_CONTEXTUAL_HINTS) };
    }
}
