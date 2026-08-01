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
pub const TOOL_META_VERSION: u32 = 3;
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
            ToolKind::WaitTasksAction => "Wait for Tasks",
            ToolKind::KillTaskAction => "Kill Task",
            ToolKind::Skill => "Skill",
            ToolKind::MemorySearch => "Memory Search",
            ToolKind::MemoryGet => "Memory Read",
            ToolKind::Task => "Subagent",
            ToolKind::PlanControl => "Plan Control",
            ToolKind::AskUser => "Ask User",
            ToolKind::DeployApp => "Deploy App",
            ToolKind::SearchTool => "Search Tools",
            ToolKind::UseTool => "Use Tool",
            ToolKind::Monitor => "Monitor",
            ToolKind::GoalUpdate => "Update Goal",
            ToolKind::Workflow => "Workflow",
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
            | ToolKind::WebFetch
            | ToolKind::PlanControl
            | ToolKind::AskUser => ToolScope::Read,
            ToolKind::Edit
            | ToolKind::Delete
            | ToolKind::Write
            | ToolKind::Move
            | ToolKind::Execute
            | ToolKind::Plan
            | ToolKind::BackgroundTaskAction
            | ToolKind::WaitTasksAction
            | ToolKind::KillTaskAction
            | ToolKind::Skill
            | ToolKind::Task
            | ToolKind::DeployApp
            | ToolKind::SearchTool
            | ToolKind::UseTool
            | ToolKind::Monitor
            | ToolKind::GoalUpdate
            | ToolKind::Workflow
            | ToolKind::Other => ToolScope::Write,
        }
    }
}
impl schemars::JsonSchema for ToolKind {
    fn schema_name() -> Cow<'static, str> {
        "ToolKind".into()
    }
    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        use strum::IntoEnumIterator;
        let known = Self::iter()
            .filter_map(|k| serde_json::to_value(k).ok())
            .filter_map(|v| v.as_str().map(|s| format!("`{s}`")))
            .collect::<Vec<_>>()
            .join(", ");
        schemars::json_schema!({
            "type": "string",
            "description": format!(
                "Categorizes what a tool does at a high level. Open set — consumers must \
                 tolerate unknown values (Rust deserializes them to `other` via \
                 `#[serde(other)]`). Known values: {known}."
            ),
        })
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
///   toolset, `list_dir` in another); prefer `label` to join, tolerate unknowns.
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
/// - **Versioning:** additive changes (new object fields, new `kind` / `label`
///   values) don't bump `version`. Unknown `kind` degrades to `"other"`;
///   `namespace` is a closed enum (no `other` sink), so a new toolset fails
///   strict typed deserialization of the whole envelope — intentional, to force
///   typed consumers with exhaustive matches to update. Out-of-tree consumers
///   should read `namespace` loosely (as a string) and, on any `grow/tool`
///   parse failure, treat it as absent and fall back to `raw_input` + the ACP
///   `kind`. `version` bumps only on removal or meaning change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
    fn unknown_kind_degrades_to_other() {
        let k: ToolKind = serde_json::from_value(serde_json::json!("teleport")).unwrap();
        assert_eq!(k, ToolKind::Other);
    }
    /// The published `kind` schema must stay an open string (codegen'd
    /// consumers would otherwise hard-fail on new kinds, contradicting the
    /// `#[serde(other)]` contract above). `namespace` stays intentionally
    /// closed — see the versioning contract on [`CanonicalToolMeta`].
    #[test]
    fn kind_schema_is_open_string_namespace_stays_closed() {
        let kind = serde_json::to_value(schemars::schema_for!(ToolKind)).unwrap();
        assert_eq!(kind["type"], "string");
        assert!(kind.get("enum").is_none(), "kind must not be a closed enum");
        assert!(
            kind["description"].as_str().unwrap().contains("`read`"),
            "known values must be listed in the description"
        );
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
