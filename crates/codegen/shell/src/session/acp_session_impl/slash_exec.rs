use super::*;
use tools::implementations::grow_build::LoopFireMode;

fn completed_goal_control_cancel_trigger(
    control: Option<&'static str>,
    before_goal: Option<(String, u64, crate::session::goal_tracker::GoalStatus)>,
    after_goal: Option<(String, u64, crate::session::goal_tracker::GoalStatus)>,
    behavior_changed: bool,
) -> Option<&'static str> {
    match control? {
        "goal_set" => (before_goal.as_ref().map(|goal| &goal.0)
            != after_goal.as_ref().map(|goal| &goal.0)
            || behavior_changed)
            .then_some("goal_set"),
        "goal_edit" => before_goal
            .zip(after_goal)
            .is_some_and(|(before, after)| before.0 == after.0 && before.1 != after.1)
            .then_some("goal_edit"),
        "goal_enter" => behavior_changed.then_some("goal_enter"),
        "goal_pause" => before_goal
            .zip(after_goal)
            .is_some_and(|(before, after)| {
                before.2 == crate::session::goal_tracker::GoalStatus::Active
                    && after.2 != crate::session::goal_tracker::GoalStatus::Active
            })
            .then_some("goal_pause"),
        "goal_clear" => (before_goal.is_some() && after_goal.is_none() || behavior_changed)
            .then_some("goal_clear"),
        _ => None,
    }
}

impl SessionActor {
    async fn queue_host_command(&self, command: String) {
        let prompt_id = format!("host-command-{}", uuid::Uuid::now_v7());
        let prompt_mode = *self.current_prompt_mode.lock();
        let (respond_to, _) = tokio::sync::oneshot::channel();
        self.state.lock().await.pending_inputs.push_back(InputItem {
            prompt_id,
            turn_kind: crate::session::TurnKind::Internal,
            prompt_blocks: vec![acp::ContentBlock::Text(acp::TextContent::new(command))],
            prompt_mode,
            client_identifier: None,
            screen_mode: None,
            verbatim: true,
            json_schema: None,
            origin: crate::session::PromptOrigin::HostCommand,
            task_wake_fallback: None,
            respond_to,
            persist_ack: None,
            parsed_prompt_tx: None,
            queue_meta: None,
        });
    }

    /// Resolve and execute a slash command from the out-of-band command plane.
    ///
    /// Only goal controls are allowed to mutate a session while a turn is in
    /// flight. Other host commands receive an explicit response instead of
    /// being reclassified as model input. A successful control that invalidates
    /// the running turn returns a structured cancellation trigger; read-only or
    /// non-invalidating controls leave the turn running.
    pub(super) async fn execute_out_of_band_slash_command(
        self: &Arc<Self>,
        command: String,
    ) -> Result<Option<&'static str>, String> {
        let command = command.trim().to_string();
        if !command.starts_with('/') {
            return Err("Grow commands must start with '/'.".to_string());
        }

        let blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(
            command.clone(),
        ))];
        let slash_skills = self.slash_skills_for_resolve().await;
        let availability = self.command_availability().await;
        let (_, workflows) = self.named_workflow_snapshot();
        let loop_fire_mode = if self.rebuild_spec.scheduler_background_loops {
            LoopFireMode::Detached
        } else {
            LoopFireMode::InSession
        };
        let action = match slash_commands::resolve(
            blocks,
            &slash_skills,
            availability,
            slash_commands::SkillSlashRewrite::RewriteToRun,
            &workflows,
            loop_fire_mode,
        ) {
            Err(SlashCommandOutcome::Builtin(action)) => action,
            Err(SlashCommandOutcome::InvokeSkill { .. }) => {
                return Err(format!(
                    "{command} starts model work and cannot run inside an active turn. Queue it for the next turn."
                ));
            }
            Ok(_) => return Err(format!("Unknown Grow command: {command}")),
        };

        match action {
            action @ (BuiltinAction::GoalSet { .. }
            | BuiltinAction::GoalEdit { .. }
            | BuiltinAction::GoalEnter
            | BuiltinAction::GoalStatus
            | BuiltinAction::GoalPause
            | BuiltinAction::GoalResume
            | BuiltinAction::GoalClear
            | BuiltinAction::GoalBudget { .. }) => {
                let control = match &action {
                    BuiltinAction::GoalSet { .. } => Some("goal_set"),
                    BuiltinAction::GoalEdit { .. } => Some("goal_edit"),
                    BuiltinAction::GoalEnter => Some("goal_enter"),
                    BuiltinAction::GoalPause => Some("goal_pause"),
                    BuiltinAction::GoalClear => Some("goal_clear"),
                    BuiltinAction::GoalStatus
                    | BuiltinAction::GoalResume
                    | BuiltinAction::GoalBudget { .. } => None,
                    _ => unreachable!("match arm accepts only Goal controls"),
                };
                let before_goal = self
                    .goal_tracker
                    .lock()
                    .snapshot()
                    .map(|goal| (goal.goal_id.clone(), goal.objective_revision, goal.status));
                let before_mode = *self.current_prompt_mode.lock();
                ::diagnostics::session_ctx::log_event(::diagnostics::events::SlashCommandUsed {
                    command: "goal".to_string(),
                    args_provided: action.args_provided(),
                });
                self.execute_builtin_slash_command(action)
                    .await
                    .map_err(|error| error.to_string())?;
                let after_goal = self
                    .goal_tracker
                    .lock()
                    .snapshot()
                    .map(|goal| (goal.goal_id.clone(), goal.objective_revision, goal.status));
                let behavior_changed = before_mode != *self.current_prompt_mode.lock();
                Ok(completed_goal_control_cancel_trigger(
                    control,
                    before_goal,
                    after_goal,
                    behavior_changed,
                ))
            }
            other => {
                ::diagnostics::session_ctx::log_event(::diagnostics::events::SlashCommandUsed {
                    command: other.command_name().to_string(),
                    args_provided: other.args_provided(),
                });
                if self.state.lock().await.foreground.regular().is_some() {
                    Err(format!(
                        "/{} cannot run inside an active turn. It was not treated as model input.",
                        other.command_name()
                    ))
                } else {
                    self.queue_host_command(command).await;
                    Ok(None)
                }
            }
        }
    }

    pub(super) fn update_goal_token_budget(&self, token_budget: Option<i64>) -> String {
        let mut tracker = self.goal_tracker.lock();
        if tracker.snapshot().is_none() {
            "当前没有活跃目标。使用 /goal set <objective> 开始。".to_string()
        } else if let Some(budget) = token_budget {
            if tracker.status() == Some(crate::session::goal_tracker::GoalStatus::Complete) {
                "Goal is already complete. Use /goal set <objective> to start a new one."
                    .to_string()
            } else {
                let was_budget_limited = tracker.status()
                    == Some(crate::session::goal_tracker::GoalStatus::BudgetLimited);
                let updated = tracker.set_token_budget(Some(budget));
                debug_assert!(updated);
                self.goal_notify_sender().persist_goal_state(&tracker);
                if was_budget_limited {
                    format!(
                        "User set current goal budget to {budget} tokens. Use /goal resume to continue."
                    )
                } else {
                    format!("User set current goal budget to {budget} tokens.")
                }
            }
        } else {
            "Usage: /goal budget <tokens>".to_string()
        }
    }

    /// Execute a built-in slash command (e.g. `/compact`, `/yolo`).
    pub(super) async fn execute_builtin_slash_command(
        self: &Arc<Self>,
        action: BuiltinAction,
    ) -> PromptTurnResult {
        ::diagnostics::session_ctx::log_event(::diagnostics::events::SlashCommandUsed {
            command: action.command_name().to_string(),
            args_provided: action.args_provided(),
        });
        match action {
            BuiltinAction::Compact { user_context } => {
                self.run_compact(user_context).await?;
                ok_end_turn(0, None)
            }
            BuiltinAction::SetYolo { enabled } => {
                let was = self.permissions.is_yolo_mode();
                self.permissions.set_yolo_mode(enabled);
                // Report the ACTUAL state, not the request: the manager clamps a
                // requested ON to OFF under the always-approve pin, so `enabled`
                // would mis-report a turn-on (event, diagnostics, and the log line)
                // that never happened.
                let actual = self.permissions.is_yolo_mode();
                if let Some(actual) = yolo_toggle_report(was, actual) {
                    self.emit_event(crate::session::events::Event::YoloToggled { enabled: actual });
                    ::diagnostics::session_ctx::log_event(::diagnostics::events::YoloToggled {
                        enabled: actual,
                        previous_state: was,
                        trigger: ::diagnostics::events::YoloTrigger::SlashCommand,
                    });
                    tracing::info_span!(
                        "session.permission_mode_changed",
                        from_mode = crate::session::diagnostics::permission_mode_label(was),
                        to_mode = crate::session::diagnostics::permission_mode_label(actual),
                        trigger = "slash_command",
                        enabled = actual,
                    )
                    .in_scope(|| {});
                }
                let status = if actual { "enabled" } else { "disabled" };
                tracing::info!(
                    session_id = %self.session_info.id.0,
                    requested = enabled,
                    enabled = actual,
                    "YOLO mode {status} via /yolo slash command",
                );
                ok_end_turn(0, None)
            }
            BuiltinAction::FlushMemory => {
                if self.memory.is_enabled() {
                    let did_flush = self.run_memory_flush("slash_command", None).await;
                    if !did_flush {
                        tracing::info!(
                            session_id = %self.session_info.id.0,
                            "memory flush skipped via /flush: another flush already in progress",
                        );
                    }
                } else {
                    tracing::warn!(
                        session_id = %self.session_info.id.0,
                        "memory flush skipped via /flush: memory not enabled for this session",
                    );
                }
                ok_end_turn(0, None)
            }
            BuiltinAction::Dream => {
                // No user-visible output — intentional, matches /flush behaviour.
                if self.memory.is_enabled() {
                    self.run_dream_slash_command().await;
                } else {
                    tracing::warn!(
                        session_id = %self.session_info.id.0,
                        "dream skipped via /dream: memory not enabled for this session",
                    );
                }
                ok_end_turn(0, None)
            }
            BuiltinAction::ContextInfo => ok_end_turn(0, None),
            BuiltinAction::HooksTrust => {
                let msg = match Self::do_hooks_trust_project(&self.session_info.cwd) {
                    Ok(root) => {
                        ::diagnostics::session_ctx::log_event(::diagnostics::events::HookTrusted {
                            success: true,
                        });
                        format!("Trusted: {}.", root.display())
                    }
                    Err(e) => {
                        ::diagnostics::session_ctx::log_event(::diagnostics::events::HookTrusted {
                            success: false,
                        });
                        e
                    }
                };
                self.send_host_turn_slash_command_output(&msg).await;
                ok_end_turn(0, None)
            }
            BuiltinAction::HooksList => {
                let text = match &*self.hook_registry.borrow() {
                    Some(registry) => {
                        let hooks = registry.all_hooks();
                        if hooks.is_empty() {
                            "No hooks loaded for this session.".to_string()
                        } else {
                            let mut lines = vec![format!("Loaded hooks ({}):", hooks.len())];
                            for spec in &hooks {
                                let matcher_str = spec
                                    .configured_matcher
                                    .as_ref()
                                    .map(|m| format!("  matcher: {m}"))
                                    .unwrap_or_default();
                                let target = if let Some(ref cmd) = spec.command {
                                    format!("command: {}", cmd.display())
                                } else if let Some(ref url) = spec.url {
                                    format!("url: {url}")
                                } else {
                                    "target: <none>".to_string()
                                };
                                lines.push(format!(
                                    "  {}{}  {}  timeout: {}s",
                                    spec.name,
                                    matcher_str,
                                    target,
                                    spec.timeout_ms / 1000,
                                ));
                            }
                            lines.join("\n")
                        }
                    }
                    None => "No hooks loaded for this session.".to_string(),
                };
                self.send_host_turn_slash_command_output(&text).await;
                ok_end_turn(0, None)
            }
            BuiltinAction::HooksAdd { path } => {
                if path.is_empty() {
                    self.send_host_turn_slash_command_output(
                        "Usage: /hooks add <path>\nProvide a path to a hook JSON file or directory under ~/.grow/.",
                    )
                    .await;
                } else {
                    // CWE-427: Use shared add_hooks_path() which validates
                    // paths are under ~/.grow/ to prevent hook path injection.
                    match crate::config::add_hooks_path(&path) {
                        Ok(()) => {
                            ::diagnostics::session_ctx::log_event(
                                ::diagnostics::events::HookAdded { success: true },
                            );
                            self.send_host_turn_slash_command_output(&format!(
                                "Added hook path: {path}\n\
                                 Restart session to load hooks from this path."
                            ))
                            .await;
                        }
                        Err(e) => {
                            ::diagnostics::session_ctx::log_event(
                                ::diagnostics::events::HookAdded { success: false },
                            );
                            self.send_host_turn_slash_command_output(&format!(
                                "Failed to add hook path: {e}"
                            ))
                            .await;
                        }
                    }
                }
                ok_end_turn(0, None)
            }
            BuiltinAction::HooksRemove { path } => {
                if path.is_empty() {
                    self.send_host_turn_slash_command_output(
                        "Usage: /hooks-remove <path>\nProvide the path to remove from hooks-paths.",
                    )
                    .await;
                } else {
                    match crate::config::remove_hooks_path(&path) {
                        Ok(()) => {
                            ::diagnostics::session_ctx::log_event(
                                ::diagnostics::events::HookRemoved { success: true },
                            );
                            self.send_host_turn_slash_command_output(&format!(
                                "Removed hook path: {path}\nRestart session to stop loading hooks from this path."
                            ))
                            .await;
                        }
                        Err(e) => {
                            ::diagnostics::session_ctx::log_event(
                                ::diagnostics::events::HookRemoved { success: false },
                            );
                            self.send_host_turn_slash_command_output(&format!(
                                "Failed to remove hook path: {e}"
                            ))
                            .await;
                        }
                    }
                }
                ok_end_turn(0, None)
            }
            BuiltinAction::HooksUntrust => {
                let msg = match Self::do_hooks_untrust_project(&self.session_info.cwd) {
                    Ok((root, true)) => format!("Untrusted: {}.", root.display()),
                    Ok((root, false)) => format!("Not currently trusted: {}", root.display()),
                    Err(e) => e,
                };
                self.send_host_turn_slash_command_output(&msg).await;
                ok_end_turn(0, None)
            }
            BuiltinAction::PluginsList => {
                let text = match &*self.plugin_registry.borrow() {
                    Some(registry) if !registry.is_empty() => {
                        let mut lines = Vec::new();
                        for plugin in registry.list() {
                            let status = if !plugin.enabled {
                                " [disabled]"
                            } else if !plugin.trusted {
                                " [untrusted]"
                            } else {
                                ""
                            };
                            let version = plugin
                                .version
                                .as_deref()
                                .map(|v| format!(" v{v}"))
                                .unwrap_or_default();
                            lines.push(format!(
                                "  {}{} ({}{})",
                                plugin.name, version, plugin.scope, status,
                            ));
                            let mut components = Vec::new();
                            if plugin.skill_count > 0 {
                                components.push(format!("{} skills", plugin.skill_count));
                            }
                            if plugin.agent_count > 0 {
                                components.push(format!("{} agents", plugin.agent_count));
                            }
                            if plugin.has_hooks {
                                components.push(if plugin.has_inline_hooks_only {
                                    "hooks: active (inline)".into()
                                } else if plugin.trusted {
                                    "hooks: active".into()
                                } else {
                                    "hooks: blocked".into()
                                });
                            }
                            if plugin.mcp_server_count > 0 {
                                components.push(if plugin.has_inline_mcp_only {
                                    format!("{} MCP servers (inline)", plugin.mcp_server_count)
                                } else if plugin.trusted {
                                    format!("{} MCP servers", plugin.mcp_server_count)
                                } else {
                                    format!("{} MCP servers: blocked", plugin.mcp_server_count)
                                });
                            }
                            if !components.is_empty() {
                                lines.push(format!("    {}", components.join(", ")));
                            }
                            if !plugin.trusted {
                                lines.push(format!(
                                    "    Run: /plugins trust {}",
                                    plugin.root.display()
                                ));
                            }
                        }
                        format!(
                            "Installed plugins ({}):\n{}",
                            registry.len(),
                            lines.join("\n")
                        )
                    }
                    _ => "No plugins installed.".to_string(),
                };
                self.send_host_turn_slash_command_output(&text).await;
                ok_end_turn(0, None)
            }
            BuiltinAction::PluginsReload => {
                match &self.plugin_registry_handle {
                    Some(handle) => {
                        // Explicit user reload: force a full local-install re-copy.
                        let msg = self.reload_plugins_impl(handle, true).await;
                        ::diagnostics::session_ctx::log_event(
                            ::diagnostics::events::PluginReloaded { success: true },
                        );
                        self.send_host_turn_slash_command_output(&msg).await;
                    }
                    None => {
                        ::diagnostics::session_ctx::log_event(
                            ::diagnostics::events::PluginReloaded { success: false },
                        );
                        self.send_host_turn_slash_command_output(
                            "No plugin registry handle available. Start a new session to discover plugins.",
                        )
                        .await;
                    }
                }
                ok_end_turn(0, None)
            }
            BuiltinAction::PluginsTrust => {
                self.send_host_turn_slash_command_output(
                    "Trust/untrust has been replaced by enable/disable. Use /plugins enable <id> instead.",
                )
                .await;
                ok_end_turn(0, None)
            }
            BuiltinAction::SessionInfo => {
                let info = self.build_session_info().await;

                let model = info.model.unwrap_or_else(|| "unknown".to_string());
                let model_line = if let Some(ref resolved) = info.resolved_model_id {
                    if resolved != &model {
                        format!("**Model:** {} ({})", model, resolved)
                    } else {
                        format!("**Model:** {}", model)
                    }
                } else {
                    format!("**Model:** {}", model)
                };
                let model_hash_line = if crate::session::acp_types::should_show_model_fingerprint(
                    info.show_model_fingerprint,
                    &model,
                ) {
                    info.model_fingerprint
                        .as_deref()
                        .map(|fp| format!("\n\n**Model Hash:** {fp}"))
                        .unwrap_or_default()
                } else {
                    String::new()
                };

                let ctx = &info.context;
                let context_pct = token_estimation::usage_percentage(ctx.used, ctx.total);

                let summary_path = crate::session::persistence::session_dir(&self.session_info)
                    .join("summary.json");
                let title = tokio::task::spawn_blocking(move || {
                    std::fs::read_to_string(&summary_path)
                        .ok()
                        .and_then(|raw| {
                            serde_json::from_str::<crate::session::persistence::Summary>(&raw).ok()
                        })
                        .map(|s| s.session_summary)
                        .filter(|s| !s.is_empty())
                })
                .await
                .ok()
                .flatten();

                let title_line = match &title {
                    Some(t) => format!("**Title:** {t}\n\n"),
                    None => String::new(),
                };

                let text = format!(
                    "{}**Session ID:** {}\n\n\
                     **Working directory:** {}\n\n\
                     {}{}\n\n\
                     **Turn:** {}\n\n\
                     **Context:** {} / {} tokens ({:.0}%)",
                    title_line,
                    self.session_info.id.0,
                    self.session_info.cwd,
                    model_line,
                    model_hash_line,
                    info.turn_index,
                    ctx.used,
                    ctx.total,
                    context_pct,
                );
                self.send_host_turn_slash_command_output(&text).await;
                ok_end_turn(0, None)
            }
            BuiltinAction::PluginsAdd { path } => {
                if path.is_empty() {
                    self.send_host_turn_slash_command_output(
                        "Usage: /plugins add <path>\n\
                         Provide the path to a plugin directory to add.",
                    )
                    .await;
                } else {
                    let resolved = {
                        let p = std::path::Path::new(&path);
                        if p.is_relative() {
                            std::path::PathBuf::from(&self.session_info.cwd).join(p)
                        } else {
                            p.to_path_buf()
                        }
                    };
                    let path_str = resolved.to_string_lossy().to_string();
                    match crate::config::add_plugin_path(&path_str) {
                        Ok(()) => {
                            ::diagnostics::session_ctx::log_event(
                                ::diagnostics::events::PluginAdded {
                                    source: ::diagnostics::events::PluginSource::LocalPath,
                                    success: true,
                                },
                            );
                            let msg = format!("Added plugin path: {path_str}");
                            self.send_host_turn_slash_command_output(&msg).await;
                            if let Some(ref handle) = self.plugin_registry_handle {
                                let reload_msg = self.reload_plugins_impl(handle, false).await;
                                self.send_host_turn_slash_command_output(&reload_msg).await;
                            }
                        }
                        Err(e) => {
                            ::diagnostics::session_ctx::log_event(
                                ::diagnostics::events::PluginAdded {
                                    source: ::diagnostics::events::PluginSource::LocalPath,
                                    success: false,
                                },
                            );
                            self.send_host_turn_slash_command_output(&format!(
                                "Failed to add plugin path: {e}"
                            ))
                            .await;
                        }
                    }
                }
                ok_end_turn(0, None)
            }
            BuiltinAction::PluginsRemove { path } => {
                if path.is_empty() {
                    self.send_host_turn_slash_command_output(
                        "Usage: /plugins remove <path>\n\
                         Provide the path to a plugin directory to remove.",
                    )
                    .await;
                } else {
                    let resolved = {
                        let p = std::path::Path::new(&path);
                        if p.is_relative() {
                            std::path::PathBuf::from(&self.session_info.cwd).join(p)
                        } else {
                            p.to_path_buf()
                        }
                    };
                    let path_str = resolved.to_string_lossy().to_string();
                    match crate::config::remove_plugin_path(&path_str) {
                        Ok(()) => {
                            ::diagnostics::session_ctx::log_event(
                                ::diagnostics::events::PluginRemoved { success: true },
                            );
                            let msg = format!("Removed plugin path: {path_str}");
                            self.send_host_turn_slash_command_output(&msg).await;
                            if let Some(ref handle) = self.plugin_registry_handle {
                                let reload_msg = self.reload_plugins_impl(handle, false).await;
                                self.send_host_turn_slash_command_output(&reload_msg).await;
                            }
                        }
                        Err(e) => {
                            ::diagnostics::session_ctx::log_event(
                                ::diagnostics::events::PluginRemoved { success: false },
                            );
                            self.send_host_turn_slash_command_output(&format!(
                                "Failed to remove plugin path: {e}"
                            ))
                            .await;
                        }
                    }
                }
                ok_end_turn(0, None)
            }
            BuiltinAction::PluginsInstall { source, trust } => {
                if source.is_empty() {
                    self.send_host_turn_slash_command_output(
                        "Usage: /plugins install <source>\n\
                         Source can be a git URL or local path.\n\
                         Examples:\n\
                           /plugins install https://github.com/user/my-plugin\n\
                           /plugins install https://github.com/user/repo@v1.0\n\
                           /plugins install ./local-plugin",
                    )
                    .await;
                } else {
                    let cwd = std::path::Path::new(&self.session_info.cwd);

                    if !trust {
                        let install_source =
                            agent::plugins::git_install::parse_install_source(&source, cwd);
                        let source_desc = match &install_source {
                            agent::plugins::git_install::InstallSource::Git { url, .. } => {
                                format!("remote git repo: {url}")
                            }
                            agent::plugins::git_install::InstallSource::Local { path, .. } => {
                                format!("local directory: {}", path.display())
                            }
                        };
                        self.send_host_turn_slash_command_output(&format!(
                            "About to install plugin from: {source_desc}\n\
                             \n\
                             This will clone/link the source and activate all executable surfaces:\n\
                               - Hook scripts will run on tool use events\n\
                               - MCP servers will be started\n\
                               - Skills will be available to the model\n\
                             \n\
                             To proceed, re-run with --trust:\n\
                               /plugins install {source} --trust"
                        ))
                        .await;
                    } else {
                        match crate::plugin::install_plugin(&source, cwd) {
                            Ok(outcome) => {
                                for w in &outcome.warnings {
                                    tracing::warn!("{w}");
                                }
                                let kind = if outcome.is_local {
                                    ::diagnostics::events::InstallKind::Local
                                } else {
                                    ::diagnostics::events::InstallKind::Git
                                };
                                ::diagnostics::session_ctx::log_event(
                                    ::diagnostics::events::PluginInstalled {
                                        install_kind: kind,
                                        success: true,
                                        trust: true,
                                        error_category: None,
                                    },
                                );
                                tracing::info_span!(
                                    "plugin.installed",
                                    success = true,
                                    install_kind = kind.as_str(),
                                    plugin_count = outcome.plugin_names.len() as i64,
                                    plugin_name = %outcome.plugin_names.join(","),
                                )
                                .in_scope(|| {});
                                self.send_host_turn_slash_command_output(&format!(
                                    "Installed {} plugin(s) from {source}: {}\n\
                                     Run /plugins reload to activate.",
                                    outcome.plugin_names.len(),
                                    outcome.plugin_names.join(", "),
                                ))
                                .await;
                            }
                            Err(e) => {
                                let error_category = Self::classify_install_error(&e);
                                let kind = if crate::plugin::install_source_is_local(&source, cwd) {
                                    ::diagnostics::events::InstallKind::Local
                                } else {
                                    ::diagnostics::events::InstallKind::Git
                                };
                                tracing::info_span!(
                                    "plugin.installed",
                                    success = false,
                                    install_kind = kind.as_str(),
                                    error_category = %error_category,
                                )
                                .in_scope(|| {});
                                ::diagnostics::session_ctx::log_event(
                                    ::diagnostics::events::PluginInstalled {
                                        install_kind: kind,
                                        success: false,
                                        trust: true,
                                        error_category: Some(error_category),
                                    },
                                );
                                self.send_host_turn_slash_command_output(&format!(
                                    "Failed to install plugin: {e}"
                                ))
                                .await;
                            }
                        }
                    }
                }
                ok_end_turn(0, None)
            }
            BuiltinAction::PluginsUninstall { name, confirm } => {
                if name.is_empty() {
                    self.send_host_turn_slash_command_output(
                        "Usage: /plugins uninstall <name>\n\
                         Provide the name of an installed plugin to remove.",
                    )
                    .await;
                } else {
                    use crate::plugin::UninstallError;
                    match crate::plugin::uninstall_plugin(&name, confirm, false) {
                        Ok(outcome) => {
                            ::diagnostics::session_ctx::log_event(
                                ::diagnostics::events::PluginUninstalled {
                                    confirmed: true,
                                    success: true,
                                },
                            );
                            self.send_host_turn_slash_command_output(&format!(
                                "Uninstalled repo \"{}\" ({} plugin(s): {})",
                                outcome.repo_key,
                                outcome.removed_plugins.len(),
                                outcome.removed_plugins.join(", "),
                            ))
                            .await;
                        }
                        Err(UninstallError::NeedsConfirm {
                            name,
                            repo_key,
                            other_plugins,
                            total,
                        }) => {
                            self.send_host_turn_slash_command_output(&format!(
                                "Plugin \"{name}\" belongs to repo \"{repo_key}\" which also contains:\n\
                                 {}\n\
                                 \n\
                                 Uninstalling will remove all {total} plugin(s). To proceed:\n\
                                   /plugins uninstall {name} --confirm\n\
                                 \n\
                                 To disable a single plugin without removing the repo, add to config.toml:\n\
                                   [plugins]\n\
                                   disabled = [\"{name}\"]",
                                other_plugins.iter().map(|p| format!("  - {p}")).collect::<Vec<_>>().join("\n"),
                            ))
                            .await;
                        }
                        Err(UninstallError::NotFound { name }) => {
                            self.send_host_turn_slash_command_output(&format!(
                                "Plugin \"{name}\" not found in install registry.\n\
                                 Use /plugins list to see installed plugins."
                            ))
                            .await;
                        }
                    }
                }
                ok_end_turn(0, None)
            }
            BuiltinAction::PluginsUpdate { name } => {
                use crate::plugin::RepoUpdateOutcome;

                match crate::plugin::update_plugins(name.as_deref()) {
                    Ok(outcomes) if outcomes.is_empty() => {
                        self.send_host_turn_slash_command_output("No installed plugins to update.")
                            .await;
                    }
                    Ok(outcomes) => {
                        fn short(c: Option<&str>) -> &str {
                            c.map(|s| &s[..7.min(s.len())]).unwrap_or("?")
                        }
                        let messages: Vec<String> = outcomes
                            .iter()
                            .map(|o| match o {
                                RepoUpdateOutcome::Updated { repo_key, old_commit, new_commit } => {
                                    format!(
                                        "{repo_key}: updated ({} -> {})",
                                        short(old_commit.as_deref()),
                                        short(new_commit.as_deref()),
                                    )
                                }
                                RepoUpdateOutcome::AlreadyUpToDate { repo_key } => {
                                    format!("{repo_key}: already up to date")
                                }
                                RepoUpdateOutcome::Pinned { repo_key, ref_name } => {
                                    format!("{repo_key}: pinned to {ref_name} (use /plugins install <url>@<new-ref> to switch)")
                                }
                                RepoUpdateOutcome::LiveLocal { repo_key } => {
                                    format!("{repo_key}: local symlink (already live, no update needed)")
                                }
                                RepoUpdateOutcome::Failed { repo_key, error } => {
                                    format!("{repo_key}: update failed: {error}")
                                }
                            })
                            .collect();
                        self.send_host_turn_slash_command_output(&messages.join("\n"))
                            .await;
                    }
                    Err(e) => {
                        self.send_host_turn_slash_command_output(&format!("{e}"))
                            .await;
                    }
                }
                ok_end_turn(0, None)
            }
            BuiltinAction::MemoryBrowse => {
                let file_infos = if let Some(ref storage) = *self.memory.storage.borrow() {
                    match storage.list_memory_files() {
                        Ok(files) => files
                            .into_iter()
                            .map(|path| {
                                let meta = match std::fs::metadata(&path) {
                                    Ok(m) => Some(m),
                                    Err(e) => {
                                        tracing::debug!(
                                            path = %path.display(),
                                            error = %e,
                                            "skipping memory file with unreadable metadata",
                                        );
                                        None
                                    }
                                };
                                crate::extensions::notification::MemoryFileInfo {
                                    source: storage.classify_source(&path).to_string(),
                                    path: path.display().to_string(),
                                    size_bytes: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                                    modified_epoch_secs: meta
                                        .and_then(|m| m.modified().ok())
                                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                        .map(|d| d.as_secs()),
                                }
                            })
                            .collect(),
                        Err(e) => {
                            tracing::warn!(
                                session_id = %self.session_info.id.0,
                                error = %e,
                                "failed to list memory files",
                            );
                            self.send_host_turn_slash_command_output(&format!(
                                "Failed to list memory files: {e}"
                            ))
                            .await;
                            vec![]
                        }
                    }
                } else {
                    self.send_host_turn_slash_command_output(
                        "Memory is not enabled for this session.",
                    )
                    .await;
                    vec![]
                };
                tracing::info!(
                    session_id = %self.session_info.id.0,
                    file_count = file_infos.len(),
                    "memory browse: listing files",
                );
                self.send_grow_notification(GrowSessionUpdate::MemoryFiles { files: file_infos })
                    .await;
                ok_end_turn(0, None)
            }
            BuiltinAction::MemoryToggle { enabled } => {
                tracing::info!(
                    session_id = %self.session_info.id.0,
                    enabled,
                    "memory toggle via /memory slash command",
                );
                let msg = if enabled && !self.memory.is_enabled() {
                    if let Some(ref params) = self.memory.backend_params {
                        let storage = crate::session::memory::MemoryStorage::new(
                            std::path::Path::new(&self.session_info.cwd),
                            None,
                        );
                        if let Err(e) = storage.ensure_initialized() {
                            tracing::warn!(error = %e, "failed to initialize memory storage on re-enable");
                            format!("Memory could not be enabled: {e}")
                        } else {
                            let backend =
                                crate::session::memory::MemoryBackendImpl::from_session_params(
                                    storage.clone(),
                                    params,
                                );
                            *self.memory.search_counter.borrow_mut() =
                                Some(backend.search_counter.clone());
                            let backend: std::sync::Arc<
                                dyn tools::types::memory_backend::MemoryBackend,
                            > = std::sync::Arc::new(backend);
                            let bridge = self.agent.borrow().tool_bridge().clone();
                            bridge.update_resource(backend.clone()).await;
                            if let Err(e) = self.register_memory_tools(&bridge).await {
                                tracing::warn!(error = %e, "memory tool registration failed during toggle");
                            }
                            *self.memory.storage.borrow_mut() = Some(storage);
                            "Memory enabled for this session.".to_owned()
                        }
                    } else {
                        "Memory cannot be enabled (not configured for this session).".to_owned()
                    }
                } else if !enabled && self.memory.is_enabled() {
                    let bridge = self.agent.borrow().tool_bridge().clone();
                    if !bridge.unregister_tool_by_name(
                        tools::implementations::memory::MEMORY_SEARCH_TOOL_NAME,
                    ) {
                        tracing::debug!("memory_search tool was not registered during unregister");
                    }
                    if !bridge.unregister_tool_by_name(
                        tools::implementations::memory::MEMORY_GET_TOOL_NAME,
                    ) {
                        tracing::debug!("memory_get tool was not registered during unregister");
                    }
                    *self.memory.storage.borrow_mut() = None;
                    *self.memory.search_counter.borrow_mut() = None;
                    "Memory disabled for this session.".to_owned()
                } else {
                    let state = if enabled { "enabled" } else { "disabled" };
                    format!("Memory is already {state}.")
                };
                self.send_host_turn_slash_command_output(&msg).await;
                self.refresh_goal_harness_enabled().await;
                ok_end_turn(0, None)
            }
            BuiltinAction::GoalSet {
                objective,
                token_budget,
            } => {
                use crate::session::behavior::BehaviorChangeOutcome;
                use crate::session::goal_tracker::GoalStatus;
                if self.behavior.lock().behavior() == Some(tool_types::BehaviorId::Goal) {
                    let message = if self
                        .goal_tracker
                        .lock()
                        .status()
                        .is_some_and(|status| status != GoalStatus::Complete)
                    {
                        "Goal is already active. Use /goal edit <objective> to revise it."
                    } else {
                        "Goal behavior is ready. Send the objective as an ordinary message."
                    };
                    self.send_host_turn_slash_command_output(message).await;
                    return ok_end_turn(0, None);
                }
                if self
                    .goal_tracker
                    .lock()
                    .status()
                    .is_some_and(|status| status != GoalStatus::Complete)
                {
                    self.send_host_turn_slash_command_output(
                        "An unfinished Goal already exists. Use /goal edit <objective>, or /goal clear first.",
                    )
                    .await;
                    return ok_end_turn(0, None);
                }
                match self
                    .request_behavior_change(acp::SessionModeId::new("goal"))
                    .await
                {
                    BehaviorChangeOutcome::Applied => {
                        self.initialize_goal_runtime(&objective, token_budget).await;
                    }
                    BehaviorChangeOutcome::ConfirmationRequired { message, .. }
                    | BehaviorChangeOutcome::Rejected { message } => {
                        self.send_host_turn_slash_command_output(&message).await;
                    }
                }
                ok_end_turn(0, None)
            }
            BuiltinAction::GoalEdit {
                objective,
                token_budget,
            } => {
                use crate::session::goal_tracker::GoalStatus;
                if self
                    .goal_tracker
                    .lock()
                    .status()
                    .is_none_or(|status| status == GoalStatus::Complete)
                {
                    self.send_host_turn_slash_command_output(
                        "No unfinished Goal can be edited. Use /goal set <objective>.",
                    )
                    .await;
                    return ok_end_turn(0, None);
                }
                let revised = self
                    .goal_tracker
                    .lock()
                    .revise_goal(objective.clone(), token_budget);
                if revised {
                    if let Some((_, cancel)) = self.goal_stage_cancel.lock().take() {
                        cancel.cancel();
                    }
                    let current = self.chat_state_handle.get_total_tokens().await as i64;
                    let (used, finished) = self.goal_tokens(current);
                    self.goal_notify_sender().emit_goal_updated(
                        &mut self.goal_tracker.lock(),
                        used,
                        finished,
                    );
                    self.idle_arbiter.notify_one();
                    self.send_host_turn_slash_command_output(&format!(
                        "Goal objective revised; background planning restarted.\nObjective: {objective}"
                    ))
                    .await;
                }
                ok_end_turn(0, None)
            }
            BuiltinAction::GoalEnter => {
                use crate::session::behavior::BehaviorChangeOutcome;
                let message = match self
                    .request_behavior_change(acp::SessionModeId::new("goal"))
                    .await
                {
                    BehaviorChangeOutcome::Applied => {
                        if self.goal_tracker.lock().snapshot().is_some() {
                            "Goal behavior selected. Use /goal status, /goal resume, or send additional context."
                                .to_string()
                        } else {
                            "Goal behavior selected. Send the objective as your next message."
                                .to_string()
                        }
                    }
                    BehaviorChangeOutcome::ConfirmationRequired { message, .. }
                    | BehaviorChangeOutcome::Rejected { message } => message,
                };
                self.send_host_turn_slash_command_output(&message).await;
                ok_end_turn(0, None)
            }
            BuiltinAction::DeepResearch { query } => {
                use crate::session::behavior::BehaviorChangeOutcome;
                let outcome = self
                    .request_behavior_change(acp::SessionModeId::new("deep_research"))
                    .await;
                match outcome {
                    BehaviorChangeOutcome::Applied if query.is_empty() => {
                        self.send_host_turn_slash_command_output(
                            "Deep Research behavior selected. Send a non-empty research query to start.",
                        )
                        .await;
                    }
                    BehaviorChangeOutcome::Applied => {
                        match self.launch_deep_research(query).await {
                            Ok(run_id) => {
                                self.send_host_turn_slash_command_output(&format!(
                                "Deep Research started in the background ({run_id}). A terminal report will be delivered here. Use /workflow to manage the run."
                            ))
                            .await;
                            }
                            Err(message) => {
                                self.send_host_turn_slash_command_output(&message).await;
                            }
                        }
                    }
                    BehaviorChangeOutcome::ConfirmationRequired { message, .. }
                    | BehaviorChangeOutcome::Rejected { message } => {
                        self.send_host_turn_slash_command_output(&message).await;
                    }
                }
                ok_end_turn(0, None)
            }
            BuiltinAction::WorkflowManage { run_id, op } => {
                let msg = self.manage_workflow_run(&run_id, &op).await;
                self.send_host_turn_slash_command_output(&msg).await;
                ok_end_turn(0, None)
            }
            BuiltinAction::WorkflowLaunch { name, input } => {
                let (registry, _) = self.named_workflow_snapshot();
                let msg = self.launch_named_workflow(&registry, &name, &input).await;
                self.send_host_turn_slash_command_output(&msg).await;
                ok_end_turn(0, None)
            }
            BuiltinAction::GoalStatus => {
                let current_tokens = self.chat_state_handle.get_total_tokens().await as i64;
                let goal_tokens = self.goal_tokens_used(current_tokens);
                let msg = {
                    let mut tracker = self.goal_tracker.lock();
                    tracker.account_elapsed();
                    match tracker.snapshot() {
                        Some(goal) => format!(
                            "Goal: {}\nStatus: {:?} | Phase: {:?}\nGoal tokens used: {}\nElapsed: {}",
                            goal.objective,
                            goal.status,
                            goal.phase,
                            goal_tokens,
                            crate::session::goal_orchestrator::format_elapsed(goal.elapsed_ms),
                        ),
                        None => "No goal is currently set. Use /goal <objective> to start one."
                            .to_string(),
                    }
                };
                self.send_host_turn_slash_command_output(&msg).await;
                ok_end_turn(0, None)
            }
            BuiltinAction::GoalPause => {
                let current_tokens = self.chat_state_handle.get_total_tokens().await as i64;
                use crate::session::goal_tracker::{GoalPauseReason, GoalStatus};
                let (msg, changed) = {
                    let mut tracker = self.goal_tracker.lock();
                    match tracker.status() {
                        Some(GoalStatus::Active) => {
                            tracker.pause(GoalPauseReason::User);
                            (
                                "User paused current goal. Use /goal resume to continue.",
                                true,
                            )
                        }
                        Some(GoalStatus::BudgetLimited) => ("Goal is budget-limited.", false),
                        Some(status) if status.is_paused() => ("Goal is already paused.", false),
                        Some(GoalStatus::Complete) => ("Goal is already complete.", false),
                        None => ("No goal is currently set.", false),
                        Some(_) => ("Goal is not active.", false),
                    }
                };
                if changed {
                    if let Some((_, cancel)) = self.goal_stage_cancel.lock().take() {
                        cancel.cancel();
                    }
                    let (tokens_used, finished) = self.goal_tokens(current_tokens);
                    self.goal_notify_sender().emit_goal_updated(
                        &mut self.goal_tracker.lock(),
                        tokens_used,
                        finished,
                    );
                }
                self.send_host_turn_slash_command_output(msg).await;
                ok_end_turn(0, None)
            }
            BuiltinAction::GoalResume => {
                use crate::session::behavior::BehaviorChangeOutcome;
                match self
                    .request_behavior_change(acp::SessionModeId::new("goal"))
                    .await
                {
                    BehaviorChangeOutcome::Applied => {
                        let message = self.resume_goal().await;
                        self.send_host_turn_slash_command_output(&message).await;
                    }
                    BehaviorChangeOutcome::ConfirmationRequired { message, .. }
                    | BehaviorChangeOutcome::Rejected { message } => {
                        self.send_host_turn_slash_command_output(&message).await;
                    }
                }
                ok_end_turn(0, None)
            }
            BuiltinAction::GoalClear => {
                if self.delete_goal_state_durably().await.is_err() {
                    self.send_host_turn_slash_command_output(
                        "Could not durably clear the goal. The goal remains loaded; retry /goal clear.",
                    )
                    .await;
                    return ok_end_turn(0, None);
                }
                if let Some((_, cancel)) = self.goal_stage_cancel.lock().take() {
                    cancel.cancel();
                }
                self.goal_tracker.lock().clear();
                self.goal_turn_task_ids.lock().clear();
                self.subagent_token_records.lock().clear();
                self.behavior.lock().select_behavior(None);
                *self.current_prompt_mode.lock() = crate::session::behavior::PromptMode::Agent;
                self.retag_queued_goal_user_prompts(crate::session::behavior::PromptMode::Agent)
                    .await;
                self.persist_behavior_state();
                self.enqueue_current_mode_update(agent_client_protocol::SessionModeId::new(
                    tools::types::SessionMode::Default.as_id(),
                ));
                self.send_grow_notification(crate::session::goal_orchestrator::build_goal_cleared())
                    .await;
                self.send_host_turn_slash_command_output("Goal cleared.")
                    .await;
                ok_end_turn(0, None)
            }
            BuiltinAction::GoalBudget { token_budget } => {
                let message = self.update_goal_token_budget(token_budget);
                self.send_host_turn_slash_command_output(&message).await;
                ok_end_turn(0, None)
            }
        }
    }
}

#[cfg(test)]
mod out_of_band_goal_control_tests {
    use super::*;
    use crate::session::goal_tracker::GoalStatus;

    fn goal(id: &str, revision: u64, status: GoalStatus) -> Option<(String, u64, GoalStatus)> {
        Some((id.to_string(), revision, status))
    }

    #[test]
    fn only_successful_invalidating_goal_controls_cancel_the_foreground() {
        assert_eq!(
            completed_goal_control_cancel_trigger(
                Some("goal_edit"),
                goal("g", 1, GoalStatus::Active),
                goal("g", 2, GoalStatus::Active),
                false,
            ),
            Some("goal_edit")
        );
        assert_eq!(
            completed_goal_control_cancel_trigger(
                Some("goal_pause"),
                goal("g", 2, GoalStatus::Active),
                goal("g", 2, GoalStatus::Paused),
                false,
            ),
            Some("goal_pause")
        );
        assert_eq!(
            completed_goal_control_cancel_trigger(
                Some("goal_clear"),
                goal("g", 2, GoalStatus::Paused),
                None,
                true,
            ),
            Some("goal_clear")
        );
        assert_eq!(
            completed_goal_control_cancel_trigger(
                Some("goal_edit"),
                goal("g", 2, GoalStatus::Active),
                goal("g", 2, GoalStatus::Active),
                false,
            ),
            None,
            "a rejected edit must not cancel unrelated work"
        );
        assert_eq!(
            completed_goal_control_cancel_trigger(
                Some("goal_set"),
                goal("g", 2, GoalStatus::Active),
                goal("g", 2, GoalStatus::Active),
                false,
            ),
            None,
            "set rejected inside Goal Behavior must not cancel its user turn"
        );
        assert_eq!(
            completed_goal_control_cancel_trigger(
                None,
                goal("g", 2, GoalStatus::Paused),
                goal("g", 2, GoalStatus::Active),
                false,
            ),
            None,
            "resume does not invalidate the running user turn"
        );
    }
}
