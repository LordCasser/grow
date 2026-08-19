use config_types::DisplayRefreshSettings;
use serde::{Deserialize, Serialize};

/// How a plain-Enter follow-up typed while a regular turn is running is
/// admitted by the shell (`queue_input`). `queue` appends to the explicit
/// user FIFO (the pre-existing behavior); `steer` auto-promotes it into the
/// running regular turn through the same mid-turn interjection path as
/// Ctrl+Enter / queue "Send now".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowUpBehavior {
    /// Append to the user FIFO (default; pre-existing behavior).
    #[default]
    Queue,
    /// Auto-promote into the running regular turn (shell-side admission).
    Steer,
}

impl FollowUpBehavior {
    /// Canonical persisted string (matches the serde wire shape).
    pub fn as_canonical(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Steer => "steer",
        }
    }

    /// Parse a canonical string. Unknown or blank values return `None` so
    /// callers fall back to the default (trim-tolerant, mirroring the
    /// appearance catalogs like `ScrollMode::from_canonical`).
    pub fn from_canonical(value: &str) -> Option<Self> {
        match value.trim() {
            "queue" => Some(Self::Queue),
            "steer" => Some(Self::Steer),
            _ => None,
        }
    }

    /// Whether this is the default value — keeps `[ui]` clean on settings
    /// writes (mirrors `ContextualHints::is_default`). `&self` so serde's
    /// `skip_serializing_if` can call it.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Lenient field deserializer: any unknown canonical string folds to the
/// default (`queue`) instead of failing the whole `[ui]` section. A
/// non-string value still fails the section (consistent with the other
/// typed `[ui]` fields, which fall back to `UiConfig::default()`).
fn deserialize_follow_up_behavior<'de, D>(deserializer: D) -> Result<FollowUpBehavior, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    Ok(FollowUpBehavior::from_canonical(&raw).unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub max_thoughts_width: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// Model ID to use for the secondary agent when forking.
    /// Empty means inherit the configured default model.
    pub fork_secondary_model: String,
    /// Compact mode. Read by pager, declared here for `serde_ignored`.
    #[serde(default)]
    pub compact_mode: bool,
    /// Simple mode. Read by pager, declared here for `serde_ignored`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simple_mode: Option<bool>,
    /// Read by `load_permission_mode()`. Declared for `serde_ignored`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// Which permission option the cursor preselects on the **first**
    /// permission prompt of a session. One of `allow_once`, `allow_always`,
    /// or `reject`. After the first prompt, the cursor sticks to the user's
    /// last-used option kind. When unset, the first prompt preselects the
    /// "Always allow on all sessions" (enable-always-approve) row. Read by
    /// the pager's permission view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_selected_permission: Option<String>,
    /// Written by the pager's appearance persist module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_timestamps: Option<bool>,
    /// Timeline sidebar (per-turn tick rail in place of the scrollbar).
    /// `None` = off (client default; opt-in). Written by the pager's settings modal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_timeline: Option<bool>,
    /// Snap a just-sent prompt to the viewport top. `None` = on (default).
    /// Written by the pager's settings modal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_flip_on_send: Option<bool>,
    /// Theme to use when the OS is in dark mode. Written by the pager's theme persist module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_dark_theme: Option<String>,
    /// Theme to use when the OS is in light mode. Written by the pager's theme persist module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_light_theme: Option<String>,
    /// Mouse-wheel and trackpad scroll speed multiplier (1–100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_speed: Option<u8>,
    /// Force scroll input classification (`auto` | `wheel` | `trackpad`).
    /// Written by the pager's settings modal; unset defaults to `auto`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_mode: Option<String>,
    /// Invert vertical scroll direction ("natural" scrolling).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invert_scroll: Option<bool>,
    /// Lines per scroll tick, applied to BOTH wheel and trackpad pricing
    /// (1–10). Unset keeps the per-terminal scroll profile's values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_lines: Option<u8>,
    /// Vim-style scrollback navigation (hjkl, gg/G, /).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vim_mode: Option<bool>,
    /// How ` ```mermaid ` code blocks are rendered (`auto` | `on` | `off`).
    /// Written by the pager's settings modal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_mermaid: Option<String>,
    /// Hunk-tracker mode the pager advertises to the agent (`agent_only` |
    /// `all_dirty` | `off`). Written by the pager's settings modal; read at
    /// connect time (CLI `--hunk-tracker-mode` / `GROW_HUNK_TRACKER` override
    /// it). `off` disables hunk tracking entirely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hunk_tracker_mode: Option<String>,
    /// When `true`, registers `Ctrl+R` (while scrollback is focused) to toggle
    /// terminal mouse reporting (mouse capture) so users can hand selection back
    /// to the terminal for native click-drag copy/paste. Opt-in only; unset/false
    /// leaves mouse reporting always on with no toggle shortcut. The prompt keeps
    /// `Ctrl+R` for history search — focus scrollback (Esc/Tab) first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mouse_reporting_toggle: Option<bool>,
    /// When cancelling a parent turn with running subagents: `always_stop` stops
    /// them without prompting, `always_continue` leaves them running without
    /// prompting. Unset/`ask` shows the cancel-turn picker. Written by the pager
    /// when the user picks "Always stop" / "Always continue".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_subagents_on_turn_cancel: Option<String>,
    /// User knob for the `remember_tool_approvals` gate: when `true`, permission
    /// prompts show the granular per-tool "Always allow …" options. Written by
    /// the settings modal; requirements/env/managed/remote settings also feed the
    /// effective gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remember_tool_approvals: Option<bool>,
    /// In-app drag selection highlight: `flash` | `hold` | `word_select`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_text_selection: Option<String>,
    /// Show agent thinking/reasoning blocks in the TUI scrollback.
    /// `None` = on (client default). Written by the pager's settings modal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_thinking_blocks: Option<bool>,
    /// Fold runs of consecutive non-destructive tool calls (reads, searches,
    /// lists) into one transcript row. `None` = on (client default). Written
    /// by the pager's settings modal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_tool_verbs: Option<bool>,
    /// Next-prompt suggestions (tab autocomplete ghost text) after each turn.
    /// `None` = on (client default). Written by the pager's settings modal;
    /// the `GROW_PROMPT_SUGGESTIONS` env var overrides at runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_suggestions: Option<bool>,
    /// Startup cursor style: `None` (default) inherits the terminal's own
    /// style; `Some(true)` forces the legacy blinking block, `Some(false)` a
    /// steady block. Config-file-only knob (no /settings row).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_blink: Option<bool>,
    /// `"fullscreen"` | `"minimal"`; unset → product default fullscreen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_mode: Option<String>,
    /// Per-tip contextual-hint opt-outs (`[ui.contextual_hints]`). Each `None`
    /// inherits the remote/default (on); `Some` is a user-explicit choice that
    /// beats the remote tier. Skipped on the wire when untouched so the section
    /// only appears once a user toggles a tip.
    #[serde(default, skip_serializing_if = "ContextualHints::is_default")]
    pub contextual_hints: ContextualHints,
    /// Combine consecutive queued follow-ups into one turn. `None` = off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combine_queued_prompts: Option<bool>,
    /// How a plain-Enter follow-up typed mid-turn is admitted: `queue`
    /// (append to the FIFO; default) or `steer` (auto-promote into the
    /// running regular turn). Read by the shell session admission
    /// (`queue_input`); no pager settings row yet (deferred — see the
    /// TodoGate precedent in `pager/src/settings/defs.rs`).
    #[serde(
        default,
        skip_serializing_if = "FollowUpBehavior::is_default",
        deserialize_with = "deserialize_follow_up_behavior"
    )]
    pub follow_up_behavior: FollowUpBehavior,
    /// Display-refresh probe + auto-cadence (`[ui.display_refresh]`). Per-field
    /// `None` inherits remote/default; skipped when untouched.
    #[serde(default, skip_serializing_if = "DisplayRefreshSettings::is_default")]
    pub display_refresh: DisplayRefreshSettings,
}

/// User-config opt-outs for the per-tip contextual hints, serialized as
/// `[ui.contextual_hints]`. Per-field `None` means "inherit remote/default";
/// `Some(bool)` is a user-explicit choice (needed so the resolver can let it
/// beat the remote tier).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextualHints {
    /// Undo tip (Ctrl+Z after a substantial draft wipe).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo: Option<bool>,
    /// Plan-mode nudge (typing a planning keyword).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_mode: Option<bool>,
    /// Clipboard-image input tip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_input: Option<bool>,
    /// Send-now tip after queuing a mid-turn follow-up (InterjectPrompt chord).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_now: Option<bool>,
    /// Small-screen tip (`/compact-mode` hint on smallish terminals).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub small_screen: Option<bool>,
    /// Word-select tip after double-clicking scrollback while Text selection
    /// is still fold/nav (`flash` / `hold`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_select: Option<bool>,
    /// SSH wrap session-load tip (recommend `grow wrap ssh` when the session
    /// runs over SSH without an OSC 52 sink).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_wrap: Option<bool>,
}

impl ContextualHints {
    /// True when no tip has a user-explicit value (all inherit). Lets the
    /// section stay absent from `config.toml` until the user toggles a tip.
    pub fn is_default(&self) -> bool {
        self.undo.is_none()
            && self.plan_mode.is_none()
            && self.image_input.is_none()
            && self.send_now.is_none()
            && self.small_screen.is_none()
            && self.word_select.is_none()
            && self.ssh_wrap.is_none()
    }
}

const DEFAULT_MAX_THOUGHTS_WIDTH: u16 = 120;

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            max_thoughts_width: DEFAULT_MAX_THOUGHTS_WIDTH,
            theme: None,
            fork_secondary_model: String::new(),
            compact_mode: false,
            simple_mode: None,
            permission_mode: None,
            default_selected_permission: None,
            show_timestamps: None,
            show_timeline: None,
            page_flip_on_send: None,
            auto_dark_theme: None,
            auto_light_theme: None,
            scroll_speed: None,
            scroll_mode: None,
            invert_scroll: None,
            scroll_lines: None,
            vim_mode: None,
            render_mermaid: None,
            hunk_tracker_mode: None,
            mouse_reporting_toggle: None,
            remember_tool_approvals: None,
            cancel_subagents_on_turn_cancel: None,
            keep_text_selection: None,
            show_thinking_blocks: None,
            group_tool_verbs: None,
            prompt_suggestions: None,
            cursor_blink: None,
            screen_mode: None,
            contextual_hints: ContextualHints::default(),
            combine_queued_prompts: None,
            follow_up_behavior: FollowUpBehavior::default(),
            display_refresh: DisplayRefreshSettings::default(),
        }
    }
}

impl UiConfig {
    /// The single source of truth for the timeline-sidebar default (opt-in).
    /// Flip this one line to change the default everywhere.
    ///
    // TODO: migrate the other boolean UI settings (show_timestamps,
    // simple_mode, show_thinking_blocks, …) to the same const + resolver
    // pattern. They currently duplicate their default literal across
    // cache.rs / config.rs / defs.rs / setters.rs / registry.rs and rely on
    // the registry drift-guard test to catch mismatches.
    pub const SHOW_TIMELINE_DEFAULT: bool = false;

    /// Resolved timeline-sidebar setting: the configured value, or
    /// [`Self::SHOW_TIMELINE_DEFAULT`] when unset. The one place the default
    /// is applied — every layer (cache, appearance config, settings modal)
    /// reads through here so they cannot drift.
    pub fn show_timeline_enabled(&self) -> bool {
        self.show_timeline.unwrap_or(Self::SHOW_TIMELINE_DEFAULT)
    }

    /// Default for [`Self::page_flip_on_send`] when unset.
    pub const PAGE_FLIP_ON_SEND_DEFAULT: bool = true;

    pub fn page_flip_on_send_enabled(&self) -> bool {
        self.page_flip_on_send
            .unwrap_or(Self::PAGE_FLIP_ON_SEND_DEFAULT)
    }

    /// True when the configured highlight mode does not timer-dismiss.
    pub fn keep_text_selection_enabled(&self) -> bool {
        matches!(
            self.keep_text_selection.as_deref(),
            Some("hold" | "word_select")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_flip_on_send_defaults_on() {
        assert!(UiConfig::default().page_flip_on_send_enabled());
        let off = UiConfig {
            page_flip_on_send: Some(false),
            ..Default::default()
        };
        assert!(!off.page_flip_on_send_enabled());
    }

    #[test]
    fn keep_text_selection_enabled_precedence() {
        let mut ui = UiConfig::default();
        assert!(!ui.keep_text_selection_enabled());

        ui.keep_text_selection = Some("flash".into());
        assert!(!ui.keep_text_selection_enabled());

        ui.keep_text_selection = Some("hold".into());
        assert!(ui.keep_text_selection_enabled());

        // `word_select` implies hold (persistent highlight).
        ui.keep_text_selection = Some("word_select".into());
        assert!(ui.keep_text_selection_enabled());
    }

    #[test]
    fn keep_text_selection_deserializes_canonical_strings() {
        let from_hold: UiConfig =
            serde_json::from_str(r#"{"keep_text_selection": "hold"}"#).unwrap();
        assert_eq!(from_hold.keep_text_selection.as_deref(), Some("hold"));

        let from_flash: UiConfig =
            serde_json::from_str(r#"{"keep_text_selection": "flash"}"#).unwrap();
        assert_eq!(from_flash.keep_text_selection.as_deref(), Some("flash"));

        assert!(serde_json::from_str::<UiConfig>(r#"{"keep_text_selection": true}"#).is_err());
    }

    #[test]
    fn display_refresh_nested_deserialize() {
        let ui: UiConfig = serde_json::from_str(
            r#"{"display_refresh": {"auto_cadence_enabled": true, "floor_ms": 7, "probe_enabled": false}}"#,
        )
        .unwrap();
        assert_eq!(ui.display_refresh.auto_cadence_enabled, Some(true));
        assert_eq!(ui.display_refresh.floor_ms, Some(7));
        assert_eq!(ui.display_refresh.probe_enabled, Some(false));
        assert!(!ui.display_refresh.is_default());
    }

    #[test]
    fn display_refresh_default_is_skipped_shape() {
        assert!(DisplayRefreshSettings::default().is_default());
        let ui = UiConfig::default();
        assert!(ui.display_refresh.is_default());
    }

    #[test]
    fn follow_up_behavior_canonicals_round_trip() {
        assert_eq!(FollowUpBehavior::Queue.as_canonical(), "queue");
        assert_eq!(FollowUpBehavior::Steer.as_canonical(), "steer");
        assert_eq!(
            FollowUpBehavior::from_canonical("queue"),
            Some(FollowUpBehavior::Queue)
        );
        assert_eq!(
            FollowUpBehavior::from_canonical(" steer "),
            Some(FollowUpBehavior::Steer)
        );
        assert_eq!(FollowUpBehavior::from_canonical("banana"), None);
        assert_eq!(FollowUpBehavior::from_canonical(""), None);
        assert!(FollowUpBehavior::Queue.is_default());
        assert!(!FollowUpBehavior::Steer.is_default());
        assert_eq!(FollowUpBehavior::default(), FollowUpBehavior::Queue);
    }

    #[test]
    fn follow_up_behavior_deserializes_unknown_string_to_default() {
        // Missing field → default queue.
        let ui: UiConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(ui.follow_up_behavior, FollowUpBehavior::Queue);
        // Explicit steer → Steer.
        let ui: UiConfig = serde_json::from_str(r#"{"follow_up_behavior": "steer"}"#).unwrap();
        assert_eq!(ui.follow_up_behavior, FollowUpBehavior::Steer);
        // Unknown canonical folds to the default instead of failing `[ui]`.
        let ui: UiConfig = serde_json::from_str(r#"{"follow_up_behavior": "banana"}"#).unwrap();
        assert_eq!(ui.follow_up_behavior, FollowUpBehavior::Queue);
    }

    #[test]
    fn follow_up_behavior_default_is_skipped_shape() {
        let serialized = serde_json::to_value(UiConfig::default()).unwrap();
        assert!(
            serialized.get("follow_up_behavior").is_none(),
            "default follow_up_behavior must not appear in serialized output"
        );
        let steer = UiConfig {
            follow_up_behavior: FollowUpBehavior::Steer,
            ..UiConfig::default()
        };
        let serialized = serde_json::to_value(&steer).unwrap();
        assert_eq!(
            serialized
                .get("follow_up_behavior")
                .and_then(|v| v.as_str()),
            Some("steer")
        );
    }
}
