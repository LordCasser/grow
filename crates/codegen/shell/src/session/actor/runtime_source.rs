use super::*;

impl SessionActor {
    /// Hash the exact source axes needed to decide whether this runtime can be
    /// restored. Only the digest is persisted; credentials and prompt text
    /// remain in memory while the canonical value is hashed.
    pub(in crate::session::actor) async fn runtime_source_hash(&self) -> Result<String, String> {
        let (definition, tool_config, authored_capability_tools, selector, plugin, role_prompt) = {
            let agent = self.agent.borrow();
            let definition = agent.definition();
            (
                serde_json::to_value(definition).map_err(|error| error.to_string())?,
                serde_json::to_value(&definition.tool_config).map_err(|error| error.to_string())?,
                definition
                    .authored_capability_tools
                    .as_ref()
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(|error| error.to_string())?,
                definition.selector_identity(),
                definition.plugin_name.clone(),
                agent.role_prompt().map(str::to_owned),
            )
        };
        let project_rules = self.agent.borrow().agents_md_section();
        let hook_registry = serde_json::to_value(self.hooks.registry.borrow().as_deref())
            .map_err(|error| error.to_string())?;
        let client_hooks = self
            .hooks
            .client_hooks
            .borrow()
            .iter()
            .map(|(event, groups)| (event.to_string(), format!("{groups:?}")))
            .collect::<std::collections::BTreeMap<_, _>>();
        let route = self.model_route.snapshot();
        let image_input_key = sampling_types::model_image_input_key_from_parts(
            &route.sampling_config.model,
            &route.sampling_config.api_backend,
            &route.sampling_config.base_url,
            &route.sampling_config.query_params,
        );
        let source = serde_json::json!({
            "projectRules": project_rules,
            "hookRegistry": hook_registry,
            "clientHooks": client_hooks,
            "workspace": self.tool_context.cwd.as_path().display().to_string(),
            "agentDefinition": definition,
            "toolConfig": tool_config,
            "authoredCapabilityTools": authored_capability_tools,
            "selector": selector,
            "maxTurns": self.max_turns,
            "taskOutputBudget": self.tool_context.task_output_token_budget.as_ref().and_then(|budget| budget.total()),
            "plugin": plugin,
            "rolePrompt": role_prompt,
            "route": {
                "modelId": route.model_id.0.to_string(),
                "modelImageInputKey": image_input_key,
                "reasoningEffort": route.sampling_config.reasoning_effort,
            },
        });
        Ok(super::tool::authorization::hash_canonical_json(&source))
    }
}
