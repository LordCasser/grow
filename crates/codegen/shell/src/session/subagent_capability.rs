//! Child-local dynamic capability state and permission-backed grant backend.

use std::collections::HashSet;
use std::sync::Arc;

use agent_client_protocol as acp;
use tools::implementations::grow_build::request_tool_access::{
    NativeCapability, RequestToolAccessInput, RequestToolAccessOutput, ToolAccessGrantBackend,
    ToolAccessGrantReason, ToolAccessGrantStatus, ToolAccessTarget,
};
use tools::implementations::grow_build::task::types::SubagentCapabilityModeExt;
use tools::types::tool::ToolKind;

pub(crate) const CAPABILITY_CATALOG_TAG: &str = "subagent-capability-catalog";

#[derive(Debug)]
struct CapabilityGrants {
    initial_mode: tool_types::SubagentCapabilityMode,
    eligible_native: HashSet<NativeCapability>,
    granted_native: HashSet<NativeCapability>,
    pending_native: HashSet<NativeCapability>,
    granted_mcp_servers: HashSet<String>,
    pending_mcp_servers: HashSet<String>,
    mcp_eligibility: Option<mcp::servers::SharedMcpEligibility>,
    bound_mcp_client_ids: std::collections::HashMap<String, u64>,
    observed_mcp_generation: u64,
}

/// In-memory authority for one live child. It is deliberately absent from
/// persistence and is never copied into nested spawn contexts.
#[derive(Debug, Clone)]
pub(crate) struct SubagentCapabilityState(Arc<parking_lot::RwLock<CapabilityGrants>>);

impl SubagentCapabilityState {
    pub(crate) async fn from_bridge(
        bridge: &tools::bridge::ToolBridge,
        authored_tools: &tools::registry::types::ToolServerConfig,
        initial_mode: tool_types::SubagentCapabilityMode,
        mcp_eligibility: Option<mcp::servers::SharedMcpEligibility>,
        bound_mcp_client_ids: std::collections::HashMap<String, u64>,
    ) -> Self {
        let mut eligible_native = HashSet::new();
        let authored_execute = authored_tools
            .tools
            .iter()
            .any(|tool| matches!(tool.kind, Some(ToolKind::Execute | ToolKind::Monitor)));
        if authored_execute
            && (bridge.tool_for_kind(ToolKind::Execute).await.is_some()
                || bridge.tool_for_kind(ToolKind::Monitor).await.is_some())
        {
            eligible_native.insert(NativeCapability::Execute);
        }
        let authored_write = authored_tools.tools.iter().any(|tool| {
            matches!(
                tool.kind,
                Some(ToolKind::Edit | ToolKind::Write | ToolKind::Delete | ToolKind::Move)
            )
        });
        let mut final_write = false;
        for kind in [
            ToolKind::Edit,
            ToolKind::Write,
            ToolKind::Delete,
            ToolKind::Move,
        ] {
            final_write |= bridge.tool_for_kind(kind).await.is_some();
        }
        if authored_write && final_write {
            eligible_native.insert(NativeCapability::ReadWrite);
        }
        let observed_mcp_generation = mcp_eligibility
            .as_ref()
            .map_or(0, mcp::servers::SharedMcpEligibility::generation);
        Self(Arc::new(parking_lot::RwLock::new(CapabilityGrants {
            initial_mode,
            eligible_native,
            granted_native: HashSet::new(),
            pending_native: HashSet::new(),
            granted_mcp_servers: HashSet::new(),
            pending_mcp_servers: HashSet::new(),
            mcp_eligibility,
            bound_mcp_client_ids,
            observed_mcp_generation,
        })))
    }

    /// Publish grants only at the sampling boundary. A model cannot forge a
    /// hidden tool call in the same response that requested the grant.
    pub(crate) fn activate_pending(&self) -> bool {
        let mut state = self.0.write();
        let pending_native = std::mem::take(&mut state.pending_native);
        let pending_mcp_servers = std::mem::take(&mut state.pending_mcp_servers);
        let changed = !pending_native.is_empty() || !pending_mcp_servers.is_empty();
        state.granted_native.extend(pending_native);
        state.granted_mcp_servers.extend(pending_mcp_servers);
        changed
    }

    pub(crate) fn allows_kind(&self, kind: ToolKind) -> bool {
        if kind == ToolKind::Other {
            return false;
        }
        if matches!(
            kind,
            ToolKind::CapabilityRequest | ToolKind::SearchTool | ToolKind::UseTool
        ) {
            return true;
        }
        let state = self.0.read();
        let required_native = if matches!(kind, ToolKind::Execute | ToolKind::Monitor) {
            Some(NativeCapability::Execute)
        } else if matches!(
            kind,
            ToolKind::Edit | ToolKind::Write | ToolKind::Delete | ToolKind::Move
        ) {
            Some(NativeCapability::ReadWrite)
        } else {
            None
        };
        if required_native.is_some_and(|capability| !state.eligible_native.contains(&capability)) {
            return false;
        }
        if state.initial_mode == tool_types::SubagentCapabilityMode::All {
            return true;
        }
        if state.initial_mode.allowed_tool_kinds().contains(&kind) {
            return true;
        }
        (state.granted_native.contains(&NativeCapability::ReadWrite)
            && tool_types::SubagentCapabilityMode::ReadWrite
                .allowed_tool_kinds()
                .contains(&kind))
            || (state.granted_native.contains(&NativeCapability::Execute)
                && tool_types::SubagentCapabilityMode::Execute
                    .allowed_tool_kinds()
                    .contains(&kind))
    }

    pub(crate) fn native_eligible(&self, capability: NativeCapability) -> bool {
        self.0.read().eligible_native.contains(&capability)
    }

    pub(crate) fn native_granted(&self, capability: NativeCapability) -> bool {
        let state = self.0.read();
        state.eligible_native.contains(&capability)
            && (state.initial_mode == tool_types::SubagentCapabilityMode::All
                || match capability {
                    NativeCapability::Execute => matches!(
                        state.initial_mode,
                        tool_types::SubagentCapabilityMode::Execute
                    ),
                    NativeCapability::ReadWrite => matches!(
                        state.initial_mode,
                        tool_types::SubagentCapabilityMode::ReadWrite
                    ),
                }
                || state.granted_native.contains(&capability))
    }

    fn native_requested_or_granted(&self, capability: NativeCapability) -> bool {
        self.native_granted(capability) || self.0.read().pending_native.contains(&capability)
    }

    pub(crate) fn mcp_server_granted(&self, server: &str) -> bool {
        let state = self.0.read();
        Self::mcp_server_eligible_locked(&state, server)
            && (state.initial_mode == tool_types::SubagentCapabilityMode::All
                || state.granted_mcp_servers.contains(server))
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

    fn mcp_server_eligible_locked(state: &CapabilityGrants, server: &str) -> bool {
        let Some(client_id) = state.bound_mcp_client_ids.get(server).copied() else {
            return false;
        };
        state
            .mcp_eligibility
            .as_ref()
            .is_some_and(|eligibility| eligibility.binding_is_current(server, client_id))
    }

    pub(crate) fn replace_bound_mcp_client_ids(
        &self,
        client_ids: std::collections::HashMap<String, u64>,
    ) {
        self.0.write().bound_mcp_client_ids = client_ids;
    }

    /// Returns true once for each parent eligibility generation observed by
    /// this child. The caller then refreshes registrations in-place before the
    /// next model sample.
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

    fn mcp_server_requested_or_granted(&self, server: &str) -> bool {
        self.mcp_server_granted(server) || self.0.read().pending_mcp_servers.contains(server)
    }

    pub(crate) fn mcp_tool_granted(&self, qualified_tool: &str) -> bool {
        self.mcp_tool_eligible(qualified_tool)
            && mcp::servers::parse_mcp_qualified_name(qualified_tool)
                .is_some_and(|(_, server, _)| self.mcp_server_granted(server))
    }

    pub(crate) fn native_catalog_prompt(&self) -> String {
        let state = self.0.read();
        let mut lines =
            vec!["Subagent capability catalog (eligibility is not authorization):".to_owned()];
        for capability in [NativeCapability::Execute, NativeCapability::ReadWrite] {
            if !state.eligible_native.contains(&capability) {
                continue;
            }
            let initially_granted = state.initial_mode == tool_types::SubagentCapabilityMode::All
                || matches!(
                    (state.initial_mode, capability),
                    (
                        tool_types::SubagentCapabilityMode::Execute,
                        NativeCapability::Execute
                    ) | (
                        tool_types::SubagentCapabilityMode::ReadWrite,
                        NativeCapability::ReadWrite
                    )
                );
            let granted = initially_granted || state.granted_native.contains(&capability);
            let name = match capability {
                NativeCapability::Execute => "execute",
                NativeCapability::ReadWrite => "read-write",
            };
            lines.push(format!(
                "- native `{name}`: {}",
                if granted {
                    "granted"
                } else {
                    "requires request_tool_access"
                }
            ));
        }
        lines.push(
            "For MCP, search_tool lists eligible server tools and their grant status. Request one mcp_server before use_tool. Every eventual Shell/Edit/MCP call still goes through permission checks."
                .to_owned(),
        );
        lines.join("\n")
    }

    fn grant(&self, target: &ToolAccessTarget) {
        let mut state = self.0.write();
        match target {
            ToolAccessTarget::Native { capability } => {
                state.pending_native.insert(*capability);
            }
            ToolAccessTarget::McpServer { server } => {
                state.pending_mcp_servers.insert(server.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(
        initial_mode: tool_types::SubagentCapabilityMode,
        eligible_native: impl IntoIterator<Item = NativeCapability>,
    ) -> SubagentCapabilityState {
        SubagentCapabilityState(Arc::new(parking_lot::RwLock::new(CapabilityGrants {
            initial_mode,
            eligible_native: eligible_native.into_iter().collect(),
            granted_native: HashSet::new(),
            pending_native: HashSet::new(),
            granted_mcp_servers: HashSet::new(),
            pending_mcp_servers: HashSet::new(),
            mcp_eligibility: None,
            bound_mcp_client_ids: Default::default(),
            observed_mcp_generation: 0,
        })))
    }

    #[test]
    fn execute_grant_is_dynamic_and_independent_from_write() {
        let state = state(
            tool_types::SubagentCapabilityMode::ReadOnly,
            [NativeCapability::Execute, NativeCapability::ReadWrite],
        );
        assert!(!state.allows_kind(ToolKind::Execute));
        assert!(!state.allows_kind(ToolKind::Edit));

        state.grant(&ToolAccessTarget::Native {
            capability: NativeCapability::Execute,
        });

        assert!(!state.allows_kind(ToolKind::Execute));
        state.activate_pending();
        assert!(state.allows_kind(ToolKind::Execute));
        assert!(!state.allows_kind(ToolKind::Edit));
    }

    #[test]
    fn write_grant_does_not_expose_execute() {
        let state = state(
            tool_types::SubagentCapabilityMode::ReadOnly,
            [NativeCapability::Execute, NativeCapability::ReadWrite],
        );
        state.grant(&ToolAccessTarget::Native {
            capability: NativeCapability::ReadWrite,
        });
        state.activate_pending();
        assert!(state.allows_kind(ToolKind::Edit));
        assert!(!state.allows_kind(ToolKind::Execute));
    }

    #[test]
    fn unknown_tool_kind_fails_closed_even_in_all_mode() {
        let state = state(tool_types::SubagentCapabilityMode::All, []);
        assert!(!state.allows_kind(ToolKind::Other));
    }

    #[test]
    fn all_mode_cannot_expose_native_tools_outside_authored_ceiling() {
        let state = state(tool_types::SubagentCapabilityMode::All, []);
        assert!(!state.allows_kind(ToolKind::Execute));
        assert!(!state.allows_kind(ToolKind::Edit));
        assert!(!state.native_granted(NativeCapability::Execute));
        assert!(!state.native_granted(NativeCapability::ReadWrite));
    }

    #[test]
    fn mcp_without_parent_eligibility_fails_closed() {
        let restricted = state(tool_types::SubagentCapabilityMode::ReadOnly, []);
        restricted.grant(&ToolAccessTarget::McpServer {
            server: "github".to_owned(),
        });
        restricted.activate_pending();
        assert!(!restricted.mcp_server_granted("github"));
    }

    #[test]
    fn mcp_grant_is_bound_to_the_inherited_transport_incarnation() {
        let mut parent = mcp::servers::McpState::new(vec![]);
        let original = Arc::new(mcp::servers::McpClient::stub("github"));
        parent
            .owned_clients
            .insert("github".to_owned(), Arc::clone(&original));
        parent.publish_eligibility(HashSet::from(["github__search".to_owned()]));
        let pool = mcp::servers::SharedMcpPool::from_state(&parent);
        let capability =
            SubagentCapabilityState(Arc::new(parking_lot::RwLock::new(CapabilityGrants {
                initial_mode: tool_types::SubagentCapabilityMode::ReadOnly,
                eligible_native: HashSet::new(),
                granted_native: HashSet::new(),
                pending_native: HashSet::new(),
                granted_mcp_servers: HashSet::new(),
                pending_mcp_servers: HashSet::new(),
                mcp_eligibility: Some(pool.eligibility()),
                bound_mcp_client_ids: std::collections::HashMap::from([(
                    "github".to_owned(),
                    original.client_id(),
                )]),
                observed_mcp_generation: 0,
            })));
        capability.grant(&ToolAccessTarget::McpServer {
            server: "github".to_owned(),
        });
        capability.activate_pending();
        assert!(capability.mcp_tool_granted("github__search"));

        parent.owned_clients.insert(
            "github".to_owned(),
            Arc::new(mcp::servers::McpClient::stub("github")),
        );
        parent.publish_eligibility(HashSet::from(["github__search".to_owned()]));

        assert!(
            !capability.mcp_tool_granted("github__search"),
            "a same-name parent reconnect must revoke the old child transport binding"
        );
    }
}

pub(crate) struct ShellToolAccessGrantBackend {
    pub state: SubagentCapabilityState,
    pub permissions: workspace::permission::PermissionHandle,
    pub request_mode: workspace::permission::types::RequestPermissionMode,
    pub session_id: String,
    pub acp_session_id: acp::SessionId,
    pub execution_cwd: std::path::PathBuf,
    pub subagent_type: Option<String>,
    pub subagent_description: Option<String>,
    pub mcp_tool_metadata: Arc<std::sync::Mutex<crate::session::tool_index::ToolMetadataSnapshot>>,
    pub pending_interactions: crate::session::pending_interaction::PendingInteractions,
    pub gateway: acp_transport::AcpAgentGatewaySender,
    pub followup_messages: Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
}

impl ShellToolAccessGrantBackend {
    fn target_available(&self, target: &ToolAccessTarget) -> bool {
        match target {
            ToolAccessTarget::Native { capability } => self.state.native_eligible(*capability),
            ToolAccessTarget::McpServer { server } => {
                self.state.mcp_server_eligible(server)
                    && self
                        .mcp_tool_metadata
                        .lock()
                        .expect("MCP tool metadata lock poisoned")
                        .tools
                        .iter()
                        .any(|tool| {
                            tool.server_name == *server
                                && self.state.mcp_tool_eligible(&tool.qualified_name)
                        })
            }
        }
    }

    fn target_granted(&self, target: &ToolAccessTarget) -> bool {
        match target {
            ToolAccessTarget::Native { capability } => {
                self.state.native_requested_or_granted(*capability)
            }
            ToolAccessTarget::McpServer { server } => {
                self.state.mcp_server_requested_or_granted(server)
            }
        }
    }
}

#[async_trait::async_trait]
impl ToolAccessGrantBackend for ShellToolAccessGrantBackend {
    async fn request(
        &self,
        input: RequestToolAccessInput,
        tool_call_id: &str,
    ) -> Result<RequestToolAccessOutput, tool_runtime::ToolError> {
        if !self.target_available(&input.target) {
            return Ok(RequestToolAccessOutput {
                status: ToolAccessGrantStatus::Unavailable,
                reason: ToolAccessGrantReason::OutsideEligibility,
                target: input.target,
                message:
                    "Requested capability is outside this subagent's hard eligibility ceiling."
                        .to_owned(),
            });
        }
        if self.target_granted(&input.target) {
            return Ok(RequestToolAccessOutput {
                status: ToolAccessGrantStatus::AlreadyGranted,
                reason: ToolAccessGrantReason::AlreadyAvailable,
                target: input.target,
                message: "Capability is already available or scheduled for the next model sample in this subagent session."
                    .to_owned(),
            });
        }

        let target_label = match &input.target {
            ToolAccessTarget::Native { capability } => format!(
                "native:{}",
                match capability {
                    NativeCapability::Execute => "execute",
                    NativeCapability::ReadWrite => "read-write",
                }
            ),
            ToolAccessTarget::McpServer { server } => format!("mcp_server:{server}"),
        };
        let raw_input = serde_json::to_value(&input).ok();
        let update = acp::ToolCallUpdate::new(
            acp::ToolCallId::new(tool_call_id.to_owned()),
            acp::ToolCallUpdateFields::new()
                .title(Some(format!("Grant subagent access to {target_label}?")))
                .kind(Some(acp::ToolKind::Other))
                .raw_input(raw_input),
        );
        let decision = {
            let _pending_guard = crate::session::pending_interaction::PendingInteractionGuard::new(
                self.pending_interactions.clone(),
                self.gateway.clone(),
                self.acp_session_id.clone(),
                tool_call_id.to_owned(),
                crate::session::pending_interaction::PendingKind::Permission,
            );
            self.permissions
                .request_with_context(
                    workspace::permission::AccessKind::CapabilityGrant {
                        target: target_label,
                        purpose: input.purpose.clone(),
                    },
                    update,
                    None,
                    workspace::permission::types::PermissionRequestContext {
                        source: workspace::permission::types::PermissionRequestSource::Child {
                            session_id: self.session_id.clone(),
                            subagent_type: self.subagent_type.clone(),
                            subagent_description: self.subagent_description.clone(),
                        },
                        request_mode: Some(self.request_mode),
                        execution_cwd: Some(self.execution_cwd.clone()),
                        // The assigned task and purpose are carried explicitly;
                        // never reuse another request's mutable transcript.
                        classifier_turns: Some(Vec::new()),
                    },
                )
                .await
        };
        if matches!(decision, workspace::permission::Decision::Allow) {
            if !self.target_available(&input.target) {
                return Ok(RequestToolAccessOutput {
                    status: ToolAccessGrantStatus::Unavailable,
                    reason: ToolAccessGrantReason::OutsideEligibility,
                    target: input.target,
                    message: "Requested capability became unavailable while permission was pending. Refresh the live capability catalog before retrying."
                        .to_owned(),
                });
            }
            self.state.grant(&input.target);
            return Ok(RequestToolAccessOutput {
                status: ToolAccessGrantStatus::Granted,
                reason: ToolAccessGrantReason::Approved,
                target: input.target,
                message: "Capability granted for this live subagent session. It becomes visible at the next model sample; actual tool calls still require permission."
                    .to_owned(),
            });
        }
        let (reason, message) = match decision {
            workspace::permission::Decision::Reject(message) => {
                (ToolAccessGrantReason::UserDenied, message)
            }
            workspace::permission::Decision::PolicyDeny(message) => {
                (ToolAccessGrantReason::PolicyDenied, message)
            }
            workspace::permission::Decision::FollowupMessage(message) => {
                self.followup_messages
                    .lock()
                    .expect("capability followup lock poisoned")
                    .insert(tool_call_id.to_owned(), message.clone());
                (ToolAccessGrantReason::FollowupRequired, message)
            }
            workspace::permission::Decision::Cancelled => (
                ToolAccessGrantReason::Cancelled,
                "Permission request cancelled.".to_owned(),
            ),
            workspace::permission::Decision::TimedOut => (
                ToolAccessGrantReason::TimedOut,
                "Permission request timed out.".to_owned(),
            ),
            workspace::permission::Decision::Ask => (
                ToolAccessGrantReason::Unresolved,
                "Permission request was not resolved.".to_owned(),
            ),
            workspace::permission::Decision::Allow => unreachable!(),
        };
        Ok(RequestToolAccessOutput {
            status: ToolAccessGrantStatus::Denied,
            reason,
            target: input.target,
            message,
        })
    }

    fn is_mcp_server_granted(&self, server: &str) -> bool {
        self.state.mcp_server_granted(server)
    }

    fn is_mcp_server_eligible(&self, server: &str) -> bool {
        self.state.mcp_server_eligible(server)
    }

    fn is_mcp_tool_eligible(&self, qualified_tool: &str) -> bool {
        self.state.mcp_tool_eligible(qualified_tool)
    }

    fn is_mcp_tool_granted(&self, qualified_tool: &str) -> bool {
        self.state.mcp_tool_granted(qualified_tool)
    }

    fn take_followup(&self, tool_call_id: &str) -> Option<String> {
        self.followup_messages
            .lock()
            .expect("capability followup lock poisoned")
            .remove(tool_call_id)
    }
}
