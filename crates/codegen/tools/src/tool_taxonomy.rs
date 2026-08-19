//! Tool taxonomy — the harness-independent vocabulary, identity, and canonical
//! `_meta` envelope.
//!
//! Depends only on `ToolKind`/`ToolNamespace` + `serde`/`serde_json` (no
//! `ToolInput`, wire codegen, or runtime). A future `tool-taxonomy` leaf crate
//! would need those two (dependency-free) enums moved here too — coherence ties
//! the inherent impls to the enum definitions. The `ToolInput`-coupled
//! projection lives in [`crate::normalization`].
use crate::types::tool::{ToolKind, ToolNamespace};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use tool_protocol::ToolScope;
/// Canonical input field names — the one vocabulary every harness normalizes
/// onto. Emit canonical keys through these so the wire contract has one source.
pub mod field {
    pub const PATH: &str = "path";
    pub const OFFSET: &str = "offset";
    pub const LIMIT: &str = "limit";
    pub const COMMAND: &str = "command";
    pub const DESCRIPTION: &str = "description";
    pub const CWD: &str = "cwd";
    pub const DIRECTORY: &str = "directory";
    pub const PATTERN: &str = "pattern";
}
/// The single `_meta` key holding the canonical tool identity as one nested
/// object (mirroring `grow/mcp_tool`). Consumers deserialize it into
/// [`CanonicalToolMeta`].
pub const TOOL_META_KEY: &str = "grow/tool";
/// Version of the canonical tool `_meta` contract. Bump on any breaking change
/// to keys or value shapes so consumers can adapt.
pub const TOOL_META_VERSION: u32 = 4;
impl ToolKind {
    /// Unified, harness-independent display label for this semantic kind. A pure
    /// function of the kind, so equivalent tools across toolsets share it
    /// (`read_file` and `Read` → `Read`; `run_terminal_cmd` and `Shell` →
    /// `Run Command`). Display only; the model's tool name is `name` in
    /// `grow/tool`. Exhaustive, so a new `ToolKind` must add a label to compile.
    pub fn presentation_name(self) -> &'static str {
        match self {
            ToolKind::Read => "Read",
            ToolKind::Edit => "Edit",
            ToolKind::Delete => "Delete",
            ToolKind::Write => "Write",
            ToolKind::Move => "Move",
            ToolKind::ListDir => "List Files",
            ToolKind::List => "List Files",
            ToolKind::Search => "Search",
            ToolKind::Lsp => "Code Intelligence",
            ToolKind::Execute => "Run Command",
            ToolKind::Plan => "Plan",
            ToolKind::WebFetch => "Web Fetch",
            ToolKind::BackgroundTaskAction => "Background Task",
            ToolKind::KillTaskAction => "Kill Task",
            ToolKind::Skill => "Skill",
            ToolKind::MemorySearch => "Memory Search",
            ToolKind::MemoryGet => "Memory Read",
            ToolKind::ContextRecall => "Recall Context",
            ToolKind::Task => "Subagent",
            ToolKind::PlanControl => "Plan Control",
            ToolKind::AskUser => "Ask User",
            ToolKind::DeployApp => "Deploy App",
            ToolKind::SearchTool => "Search Tools",
            ToolKind::UseTool => "Use Tool",
            ToolKind::Monitor => "Monitor",
            ToolKind::GoalRead => "Read Goal",
            ToolKind::GoalProgressUpdate => "Update Goal Progress",
            ToolKind::GoalReplanRequest => "Request Goal Replan",
            ToolKind::GoalLifecycleUpdate => "Update Goal Lifecycle",
            ToolKind::GoalPlanSubmit => "Submit Goal Plan",
            ToolKind::Workflow => "Workflow",
            ToolKind::CapabilityRequest => "Request Tool Access",
            ToolKind::Other => "Tool",
        }
    }
    /// Whether this kind only reads (no workspace or external mutation) by
    /// default. The kind-level default for `ToolMetadata::tool_scope`, which
    /// individual tools may override. Exhaustive (no `_`) so a new kind must
    /// classify itself rather than silently defaulting to "mutating".
    pub fn default_scope(self) -> ToolScope {
        match self {
            ToolKind::Read
            | ToolKind::Search
            | ToolKind::Lsp
            | ToolKind::ListDir
            | ToolKind::List
            | ToolKind::MemorySearch
            | ToolKind::MemoryGet
            | ToolKind::ContextRecall
            | ToolKind::GoalRead
            | ToolKind::WebFetch
            | ToolKind::GoalPlanSubmit
            | ToolKind::PlanControl
            | ToolKind::CapabilityRequest
            | ToolKind::AskUser => ToolScope::Read,
            ToolKind::Edit
            | ToolKind::Delete
            | ToolKind::Write
            | ToolKind::Move
            | ToolKind::Execute
            | ToolKind::Plan
            | ToolKind::BackgroundTaskAction
            | ToolKind::KillTaskAction
            | ToolKind::Skill
            | ToolKind::Task
            | ToolKind::DeployApp
            | ToolKind::SearchTool
            | ToolKind::UseTool
            | ToolKind::Monitor
            | ToolKind::GoalProgressUpdate
            | ToolKind::GoalReplanRequest
            | ToolKind::GoalLifecycleUpdate
            | ToolKind::Workflow
            | ToolKind::Other => ToolScope::Write,
        }
    }
}
/// Canonical identity for a tool call, resolved from a tool's registered
/// metadata by its client-facing wire name.
///
/// Harness-independent. `tool_kind` is the authoritative `metadata.kind()`.
#[derive(Debug, Clone, Copy)]
pub struct ToolIdentity {
    pub tool_kind: ToolKind,
    pub namespace: ToolNamespace,
    pub presentation_name: &'static str,
    pub scope: ToolScope,
}
/// The canonical tool-identity envelope, attached to a tool-call event `_meta`
/// as one nested object under [`TOOL_META_KEY`].
///
/// ```json
/// "grow/tool": {
///   "version": 3,
///   "name": "read_file",
///   "kind": "read",
///   "namespace": "grow",
///   "label": "Read",
///   "scope": "read",
///   "input": { "path": "..." }
/// }
/// ```
///
/// Consumer contract:
/// - **`label`** is the cross-harness grouping/display key: equivalent tools
///   share it (grow `read_file` → `"Read"`).
/// - **`kind`** is a finer discriminator (`metadata.kind()`), *not* guaranteed
///   equal for equivalent ops across harnesses (listing is `list` in one
///   toolset, `list_dir` in another); prefer `label` to join.
/// - **`name`** is the harness-specific model-facing name; for diagnostics.
///   For harness-initiated events (e.g. the `bash_mode` marker), `raw_input`
///   is not guaranteed to match `name`'s schema.
/// - **`input`** is a canonical *projection*, not a mirror: cross-harness keys
///   only, so some raw fields are intentionally dropped (e.g. grep flags,
///   `replace_all`), and bulky payload
///   fields (edit `old_string`/`new_string`, full write contents) are never
///   projected — read them from `raw_input`. It is omitted entirely
///   when no stable shape exists (MCP / dynamic / out-of-scope). When a field or
///   the whole dict is absent, fall back to `raw_input` on this or an earlier
///   update for the same `tool_call_id` (some updates, e.g. a parse failure,
///   carry neither and rely on the merge below).
/// - **Lifecycle:** updates for one call share a `tool_call_id` — merge across
///   them (last write wins); `input` may arrive on a later update.
/// - **Versioning:** `kind`, `namespace`, and the envelope fields are closed.
///   Any vocabulary or shape change bumps `version`, forcing typed consumers to
///   update rather than silently reclassifying a tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CanonicalToolMeta {
    pub version: u32,
    pub name: String,
    pub kind: ToolKind,
    pub namespace: ToolNamespace,
    pub label: Cow<'static, str>,
    pub scope: ToolScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
}
impl CanonicalToolMeta {
    /// Build from a resolved identity + already-projected `input`. `ToolInput`-free
    /// so the type stays a leaf (projection lives in `normalization`).
    pub fn new(
        name: impl Into<String>,
        identity: &ToolIdentity,
        input: Option<serde_json::Value>,
    ) -> Self {
        Self {
            version: TOOL_META_VERSION,
            name: name.into(),
            kind: identity.tool_kind,
            namespace: identity.namespace,
            label: Cow::Borrowed(identity.presentation_name),
            scope: identity.scope,
            input,
        }
    }
    /// Attach under [`TOOL_META_KEY`], preserving existing `_meta` keys
    /// (`bash_mode`, `backend`, `grow/mcp_tool`, …).
    pub fn merge_into(&self, existing: Option<serde_json::Value>) -> serde_json::Value {
        debug_assert!(
            matches!(existing, None | Some(serde_json::Value::Object(_))),
            "_meta is always absent or an object"
        );
        let mut map = match existing {
            Some(serde_json::Value::Object(m)) => m,
            Some(other) => return other,
            None => serde_json::Map::new(),
        };
        let value = serde_json::to_value(self).expect("CanonicalToolMeta serializes");
        map.insert(TOOL_META_KEY.to_string(), value);
        serde_json::Value::Object(map)
    }
}
/// The published JSON Schema (draft-07) for the [`CanonicalToolMeta`] wire
/// envelope (`schema/tool_meta.schema.json`). Non-Rust consumers codegen from
/// it; kept in sync with the type by `tool_meta_schema_is_up_to_date`.
pub fn tool_meta_json_schema_str() -> &'static str {
    include_str!("../schema/tool_meta.schema.json")
}
#[cfg(test)]
mod tests {
    use super::*;
    fn identity(kind: ToolKind) -> ToolIdentity {
        ToolIdentity {
            tool_kind: kind,
            namespace: ToolNamespace::Grow,
            presentation_name: kind.presentation_name(),
            scope: kind.default_scope(),
        }
    }
    #[test]
    fn default_scope_classifies_kinds() {
        assert_eq!(ToolKind::Read.default_scope(), ToolScope::Read);
        assert_eq!(ToolKind::Search.default_scope(), ToolScope::Read);
        assert_eq!(ToolKind::List.default_scope(), ToolScope::Read);
        assert_eq!(ToolKind::Edit.default_scope(), ToolScope::Write);
        assert_eq!(ToolKind::Execute.default_scope(), ToolScope::Write);
        assert_eq!(ToolKind::Delete.default_scope(), ToolScope::Write);
    }
    #[test]
    fn namespace_round_trips_canonical_wire_values() {
        use strum::IntoEnumIterator;
        fn wire(ns: ToolNamespace) -> &'static str {
            match ns {
                ToolNamespace::Grow => "grow",
                ToolNamespace::GrowConcise => "grow_concise",
                ToolNamespace::GrowHashline => "grow_hashline",
                ToolNamespace::MCP => "mcp",
            }
        }
        for ns in ToolNamespace::iter() {
            let wire = wire(ns);
            assert_eq!(serde_json::to_value(ns).unwrap(), serde_json::json!(wire));
            assert_eq!(
                serde_json::from_value::<ToolNamespace>(serde_json::json!(wire)).unwrap(),
                ns
            );
        }
    }
    #[test]
    fn unknown_kind_is_rejected() {
        assert!(serde_json::from_value::<ToolKind>(serde_json::json!("teleport")).is_err());
    }
    /// Both taxonomy dimensions are closed vocabularies.
    #[test]
    fn kind_and_namespace_schemas_are_closed() {
        let kind = serde_json::to_value(schemars::schema_for!(ToolKind)).unwrap();
        assert!(kind.get("enum").is_some(), "kind must be a closed enum");
        let ns = serde_json::to_value(schemars::schema_for!(ToolNamespace)).unwrap();
        assert!(ns.get("enum").is_some(), "namespace is a closed enum");
    }
    #[test]
    fn canonical_meta_wire_shape_round_trips() {
        let meta = CanonicalToolMeta::new(
            "read_file",
            &identity(ToolKind::Read),
            Some(serde_json::json!({ "path": "/a" })),
        );
        let t = serde_json::to_value(&meta).unwrap();
        assert_eq!(t["version"], serde_json::json!(TOOL_META_VERSION));
        assert_eq!(t["name"], "read_file");
        assert_eq!(t["kind"], "read");
        assert_eq!(t["namespace"], "grow");
        assert_eq!(t["label"], "Read");
        assert_eq!(t["scope"], "read");
        assert_eq!(t["input"]["path"], "/a");
        assert_eq!(
            serde_json::from_value::<CanonicalToolMeta>(t).unwrap(),
            meta
        );
    }
    /// The checked-in schema (the artifact non-Rust consumers codegen from) must
    /// track the type. Regenerate with `UPDATE_TOOL_META_SCHEMA=1`.
    #[test]
    fn tool_meta_schema_is_up_to_date() {
        let generator = schemars::generate::SchemaSettings::draft07().into_generator();
        let schema = serde_json::to_value(generator.into_root_schema_for::<CanonicalToolMeta>())
            .expect("schema serializes");
        let generated = format!("{}\n", serde_json::to_string_pretty(&schema).unwrap());
        if std::env::var("UPDATE_TOOL_META_SCHEMA").is_ok() {
            std::fs::write(
                concat!(env!("CARGO_MANIFEST_DIR"), "/schema/tool_meta.schema.json"),
                &generated,
            )
            .unwrap();
            return;
        }
        let mut expected: serde_json::Value =
            serde_json::from_str(tool_meta_json_schema_str()).expect("checked-in schema parses");
        if let Some(values) = expected["definitions"]["ToolNamespace"]["enum"].as_array_mut() {
            use std::collections::HashSet;
            use strum::IntoEnumIterator;
            let compiled: HashSet<String> = ToolNamespace::iter()
                .filter_map(|ns| {
                    serde_json::to_value(ns)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                })
                .collect();
            values.retain(|v| matches!(v.as_str(), Some(s) if compiled.contains(s)));
        }
        let expected = format!("{}\n", serde_json::to_string_pretty(&expected).unwrap());
        assert_eq!(
            generated, expected,
            "tool_meta.schema.json is stale; regenerate with UPDATE_TOOL_META_SCHEMA=1"
        );
    }
    #[test]
    fn merge_into_nests_under_one_key_and_preserves_existing() {
        let meta = CanonicalToolMeta::new("run_terminal_cmd", &identity(ToolKind::Execute), None);
        let merged = meta.merge_into(Some(serde_json::json!({"bash_mode": true})));
        let o = merged.as_object().unwrap();
        assert_eq!(o["bash_mode"], true, "existing meta must be preserved");
        let t = &o[TOOL_META_KEY];
        assert_eq!(t["kind"], "execute");
        assert!(t.get("input").is_none(), "absent input omitted");
    }
}
