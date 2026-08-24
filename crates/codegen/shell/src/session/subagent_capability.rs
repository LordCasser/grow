//! Child-local hard eligibility and immutable delegated RWX authority.

use std::collections::HashMap;
use std::sync::Arc;

use tools::types::tool::ToolKind;

pub(crate) const CAPABILITY_CATALOG_TAG: &str = "subagent-capability-catalog";

/// Immutable authority a live child may delegate to descendants. Transport
/// membership is retained for every initial mode; the descendant's RWX
/// intersection and each server's live trust-domain mask still gate calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DelegableCapabilityCeiling {
    initial_mode: tool_types::SubagentCapabilityMode,
    initial_mcp_bindings: HashMap<String, u64>,
}

impl DelegableCapabilityCeiling {
    pub(crate) fn new(
        initial_mode: tool_types::SubagentCapabilityMode,
        initial_mcp_bindings: HashMap<String, u64>,
    ) -> Self {
        Self {
            initial_mode,
            initial_mcp_bindings,
        }
    }

    pub(crate) fn constrain_mode(
        &self,
        requested: tool_types::SubagentCapabilityMode,
    ) -> tool_types::SubagentCapabilityMode {
        requested.intersection(self.initial_mode)
    }

    pub(crate) fn permits_mcp_binding(&self, server: &str, client_id: u64) -> bool {
        self.initial_mcp_bindings.get(server) == Some(&client_id)
    }
}

#[derive(Debug)]
struct CapabilityAuthority {
    authorization_epoch: u64,
    initial_mode: tool_types::SubagentCapabilityMode,
    /// Every native tool visible to the model, keyed by exact wire identity.
    visible_native: HashMap<String, (ToolKind, tool_protocol::ToolAccess)>,
    /// Immutable authored/harness eligibility ceiling.
    eligible_native: HashMap<String, tool_protocol::ToolAccess>,
    /// Live upstream membership/tool authority for inherited MCP transports.
    mcp_eligibility: Option<mcp::servers::SharedMcpEligibility>,
    bound_mcp_client_ids: HashMap<String, u64>,
    observed_mcp_generation: u64,
}

/// In-memory authority for one live child. It is never persisted or copied as
/// a runtime permit. The only mutable fields track live MCP incarnation and
/// discovery generations; Ask/Auto approvals do not widen this state.
#[derive(Debug, Clone)]
pub(crate) struct SubagentCapabilityState(Arc<parking_lot::RwLock<CapabilityAuthority>>);

fn native_descriptor_is_eligible(
    authored: bool,
    kind: ToolKind,
    max_access: tool_protocol::ToolAccess,
) -> bool {
    let intrinsic_control = matches!(
        kind,
        ToolKind::SearchTool
            | ToolKind::UseTool
            | ToolKind::Plan
            | ToolKind::PlanControl
            | ToolKind::AskUser
            // Any background-producing tool must retain its inverse
            // lifecycle operation. Kill remains safe because its injected
            // backends are session-bound and re-check owner.
            | ToolKind::KillTaskAction
    );
    // Read-only conveniences retain the historical child experience, but
    // `None` is not an automatic grant: framework controls need either an
    // authored identity or the narrow intrinsic allowlist above. This keeps
    // Task hard-forbidden at max depth.
    authored || max_access == tool_protocol::ToolAccess::Read || intrinsic_control
}

impl SubagentCapabilityState {
    pub(crate) async fn from_bridge(
        bridge: &tools::bridge::ToolBridge,
        authored_tools: &tools::registry::types::ToolServerConfig,
        initial_mode: tool_types::SubagentCapabilityMode,
        mcp_eligibility: Option<mcp::servers::SharedMcpEligibility>,
        bound_mcp_client_ids: HashMap<String, u64>,
    ) -> Self {
        let authored_names = bridge.authored_native_tool_names(authored_tools);
        let mut visible_native = HashMap::new();
        let mut eligible_native = HashMap::new();
        for (name, kind, max_access) in bridge.native_tool_descriptors() {
            visible_native.insert(name.clone(), (kind, max_access));
            if native_descriptor_is_eligible(authored_names.contains(&name), kind, max_access) {
                eligible_native.insert(name, max_access);
            }
        }
        let observed_mcp_generation = mcp_eligibility
            .as_ref()
            .map_or(0, mcp::servers::SharedMcpEligibility::generation);
        Self(Arc::new(parking_lot::RwLock::new(CapabilityAuthority {
            authorization_epoch: 0,
            initial_mode,
            visible_native,
            eligible_native,
            mcp_eligibility,
            bound_mcp_client_ids,
            observed_mcp_generation,
        })))
    }

    pub(crate) fn authorization_epoch(&self) -> u64 {
        self.0.read().authorization_epoch
    }

    fn mode_access(mode: tool_types::SubagentCapabilityMode) -> tool_protocol::ToolAccess {
        match mode {
            tool_types::SubagentCapabilityMode::ReadOnly => tool_protocol::ToolAccess::Read,
            tool_types::SubagentCapabilityMode::ReadWrite => tool_protocol::ToolAccess::ReadWrite,
            tool_types::SubagentCapabilityMode::Execute => tool_protocol::ToolAccess::ReadExecute,
            tool_types::SubagentCapabilityMode::All => tool_protocol::ToolAccess::All,
        }
    }

    fn effective_access_locked(state: &CapabilityAuthority) -> tool_protocol::ToolAccess {
        Self::mode_access(state.initial_mode)
    }

    /// Hard eligibility by exact identity and the projected RWX of this call.
    pub(crate) fn native_call_eligible(
        &self,
        tool_name: &str,
        required_access: tool_protocol::ToolAccess,
    ) -> bool {
        self.0
            .read()
            .eligible_native
            .get(tool_name)
            .is_some_and(|max| max.covers(required_access))
    }

    /// Whether immutable initial RWX already covers this eligible call.
    pub(crate) fn native_call_available(
        &self,
        tool_name: &str,
        required_access: tool_protocol::ToolAccess,
    ) -> bool {
        let state = self.0.read();
        state.eligible_native.get(tool_name).is_some_and(|max| {
            max.covers(required_access)
                && Self::effective_access_locked(&state).covers(required_access)
        })
    }

    pub(crate) fn mcp_server_eligible(&self, server: &str) -> bool {
        Self::mcp_server_eligible_locked(&self.0.read(), server)
    }

    pub(crate) fn mcp_tool_eligible(&self, qualified_tool: &str) -> bool {
        let state = self.0.read();
        let Some((_, server, _)) = mcp::servers::parse_mcp_qualified_name(qualified_tool) else {
            return false;
        };
        Self::mcp_server_eligible_locked(&state, server)
            && state
                .mcp_eligibility
                .as_ref()
                .is_some_and(|eligibility| eligibility.contains_tool(qualified_tool))
    }

    pub(crate) fn mcp_tool_available(
        &self,
        qualified_tool: &str,
        required_access: tool_protocol::ToolAccess,
    ) -> bool {
        self.mcp_tool_eligible(qualified_tool)
            && Self::effective_access_locked(&self.0.read()).covers(required_access)
    }

    fn mcp_server_eligible_locked(state: &CapabilityAuthority, server: &str) -> bool {
        let Some(client_id) = state.bound_mcp_client_ids.get(server).copied() else {
            return false;
        };
        state
            .mcp_eligibility
            .as_ref()
            .is_some_and(|eligibility| eligibility.binding_is_current(server, client_id))
    }

    pub(crate) fn replace_bound_mcp_client_ids(&self, client_ids: HashMap<String, u64>) {
        let mut state = self.0.write();
        if state.bound_mcp_client_ids != client_ids {
            state.bound_mcp_client_ids = client_ids;
            state.authorization_epoch = state.authorization_epoch.wrapping_add(1);
        }
    }

    /// Returns true once for each parent eligibility generation observed by
    /// this child. The caller refreshes registrations before the next sample.
    pub(crate) fn take_mcp_eligibility_change(&self) -> bool {
        let mut state = self.0.write();
        let Some(eligibility) = state.mcp_eligibility.as_ref() else {
            return false;
        };
        let generation = eligibility.generation();
        if generation == state.observed_mcp_generation {
            return false;
        }
        state.observed_mcp_generation = generation;
        true
    }

    pub(crate) fn native_catalog_prompt(&self) -> String {
        let state = self.0.read();
        let initial = Self::effective_access_locked(&state);
        let mut lines = vec![
            "Subagent capability catalog (visibility is not authorization):".to_owned(),
            format!("- initial RWX: {initial:?}"),
        ];
        let mut available = Vec::new();
        let mut call_projected = Vec::new();
        for (name, max_access) in &state.eligible_native {
            let rendered = format!("{name}({max_access:?})");
            if initial.covers(*max_access) {
                available.push(rendered);
            } else {
                call_projected.push(rendered);
            }
        }
        available.sort();
        call_projected.sort();
        if !available.is_empty() {
            lines.push(format!(
                "- available native tools: {}",
                available.join(", ")
            ));
        }
        if !call_projected.is_empty() {
            lines.push(format!(
                "- call-projected native tools (exact arguments decide whether initial RWX covers the call; otherwise it enters Ask/Auto): {}",
                call_projected.join(", ")
            ));
        }
        let mut forbidden = state
            .visible_native
            .keys()
            .filter(|name| !state.eligible_native.contains_key(*name))
            .cloned()
            .collect::<Vec<_>>();
        forbidden.sort();
        if !forbidden.is_empty() {
            lines.push(format!(
                "- forbidden native tools (visible for truthful discovery, never permit-able): {}",
                forbidden.join(", ")
            ));
        }
        lines.push(
            "For MCP, search_tool lists live eligible server tools. Invoke the exact use_tool call directly: calls inside initial RWX pass normally; locked calls enter Ask/Auto and can receive only a one-shot permit bound to arguments and transport generation."
                .to_owned(),
        );
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(
        initial_mode: tool_types::SubagentCapabilityMode,
        eligible_native: impl IntoIterator<Item = (&'static str, tool_protocol::ToolAccess)>,
    ) -> SubagentCapabilityState {
        let eligible_native = eligible_native
            .into_iter()
            .map(|(name, access)| (name.to_owned(), access))
            .collect::<HashMap<_, _>>();
        SubagentCapabilityState(Arc::new(parking_lot::RwLock::new(CapabilityAuthority {
            authorization_epoch: 0,
            initial_mode,
            visible_native: eligible_native
                .iter()
                .map(|(name, access)| (name.clone(), (ToolKind::Other, *access)))
                .collect(),
            eligible_native,
            mcp_eligibility: None,
            bound_mcp_client_ids: HashMap::new(),
            observed_mcp_generation: 0,
        })))
    }

    #[test]
    fn delegation_intersects_rwx_but_retains_exact_transport_ceiling() {
        let ceiling = DelegableCapabilityCeiling::new(
            tool_types::SubagentCapabilityMode::ReadWrite,
            HashMap::from([("github".to_owned(), 7)]),
        );
        assert_eq!(
            ceiling.constrain_mode(tool_types::SubagentCapabilityMode::Execute),
            tool_types::SubagentCapabilityMode::ReadOnly
        );
        assert!(ceiling.permits_mcp_binding("github", 7));
        assert!(!ceiling.permits_mcp_binding("github", 8));
    }

    #[test]
    fn exact_identity_separates_available_locked_and_forbidden() {
        let authority = state(
            tool_types::SubagentCapabilityMode::ReadWrite,
            [
                ("edit", tool_protocol::ToolAccess::ReadWrite),
                ("bash", tool_protocol::ToolAccess::All),
            ],
        );
        assert!(authority.native_call_available("edit", tool_protocol::ToolAccess::Write));
        assert!(authority.native_call_eligible("bash", tool_protocol::ToolAccess::ReadExecute));
        assert!(!authority.native_call_available("bash", tool_protocol::ToolAccess::ReadExecute));
        assert!(!authority.native_call_eligible("forged", tool_protocol::ToolAccess::Read));
    }

    #[test]
    fn execute_and_write_branches_remain_incomparable() {
        let execute = state(
            tool_types::SubagentCapabilityMode::Execute,
            [
                ("bash", tool_protocol::ToolAccess::All),
                ("edit", tool_protocol::ToolAccess::ReadWrite),
            ],
        );
        assert!(execute.native_call_available("bash", tool_protocol::ToolAccess::ReadExecute));
        assert!(!execute.native_call_available("edit", tool_protocol::ToolAccess::Write));
    }

    #[test]
    fn transport_rebind_changes_actor_epoch() {
        let authority = state(tool_types::SubagentCapabilityMode::ReadOnly, []);
        let before = authority.authorization_epoch();
        authority.replace_bound_mcp_client_ids(HashMap::from([("github".to_owned(), 9)]));
        assert_ne!(authority.authorization_epoch(), before);
    }

    #[test]
    fn task_needs_authored_identity_but_owner_cleanup_is_intrinsic() {
        use tool_protocol::ToolAccess;

        assert!(native_descriptor_is_eligible(
            true,
            ToolKind::Task,
            ToolAccess::None,
        ));
        assert!(!native_descriptor_is_eligible(
            false,
            ToolKind::Task,
            ToolAccess::None,
        ));
        assert!(native_descriptor_is_eligible(
            false,
            ToolKind::KillTaskAction,
            ToolAccess::None,
        ));
    }
}
