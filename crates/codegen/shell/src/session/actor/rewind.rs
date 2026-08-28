//! Rewind concern for `SessionActor`: rewind points, cross-compaction
//! replay detection, and `handle_rewind`.

use super::*;

impl SessionActor {
    pub(super) async fn close_rewind_window(&self) {
        let mut state = self.state.lock().await;
        state.rewindable = false;
    }

    /// Returns the `prompt_index → num_file_snapshots` map from the on-disk
    /// snapshot index (independent of the chat-state prompt index). The bridge
    /// joins these onto the server's rewind points.
    pub(super) async fn rewind_file_counts(&self) -> std::collections::HashMap<usize, usize> {
        self.file_state_tracker
            .get_rewind_point_metas()
            .await
            .into_iter()
            .map(|m| (m.prompt_index, m.num_file_snapshots))
            .collect()
    }

    /// Get available rewind points for this session.
    ///
    /// Every prompt is a checkpoint — the list always contains `[0, 1, ..., N-1]`
    /// where N is the current prompt_index. File snapshots may or may not exist
    /// for each checkpoint (indicated by `has_file_changes`).
    pub(super) async fn get_rewind_points(&self) -> RewindPointsResponse {
        // Metadata only — don't materialize the (huge) file-content snapshots
        // just to render the picker.
        let file_metas = self.file_state_tracker.get_rewind_point_metas().await;

        // Query prompt state from the chat state actor.
        let snapshot = self.chat_state_handle.snapshot().await;
        let (prompts, current_prompt_index) = match snapshot {
            Some(ref s) => (
                s.prompt_records
                    .iter()
                    .map(|record| (record.prompt_index, record.text.as_str()))
                    .collect::<std::collections::HashMap<_, _>>(),
                s.prompt_index,
            ),
            None => (std::collections::HashMap::new(), 0),
        };

        // Build a lookup of which prompt indices have file snapshots.
        let file_meta_map: std::collections::HashMap<
            usize,
            &workspace::session::file_state::RewindPointMeta,
        > = file_metas.iter().map(|m| (m.prompt_index, m)).collect();

        // Generate a rewind point for every prompt 0..current_prompt_index.
        let rewind_points = (0..current_prompt_index)
            .map(|idx| {
                let prompt_preview = prompts.get(&idx).and_then(|text| {
                    let clean_text = extract_user_query(text);
                    let first_line = clean_text
                        .lines()
                        .map(|l| l.trim())
                        .find(|l| !l.is_empty())
                        .unwrap_or("");

                    if first_line.is_empty() {
                        None
                    } else if first_line.chars().count() > 60 {
                        Some(format!("{}...", crate::util::truncate(first_line, 57)))
                    } else {
                        Some(first_line.to_string())
                    }
                });

                let file_meta = file_meta_map.get(&idx);
                let num_file_snapshots = file_meta.map_or(0, |m| m.num_file_snapshots);
                let created_at = file_meta
                    .map(|m| m.created_at.to_rfc3339())
                    .unwrap_or_default();

                RewindPointInfo {
                    prompt_index: idx,
                    created_at,
                    num_file_snapshots,
                    has_file_changes: num_file_snapshots > 0,
                    prompt_preview,
                }
            })
            .collect();

        RewindPointsResponse { rewind_points }
    }

    /// Handle a rewind request with mode support.
    ///
    /// Semantics: "restore state before prompt N ran" — prompts 0..N-1 are kept.
    ///
    /// Modes:
    /// - `All`: roll back both conversation and files (full time-travel)
    /// - `ConversationOnly`: roll back conversation, leave files untouched
    /// - `FilesOnly`: roll back files, leave conversation untouched
    pub(super) async fn handle_rewind(
        &self,
        request: RewindRequest,
    ) -> anyhow::Result<RewindResponse> {
        // Goal is durable control state independent from prompt history and
        // file rewind points. Without prompt-indexed Goal snapshots, rewinding
        // either side would make the objective's evidence boundary ambiguous.
        // Require an explicit clear instead of inventing partial rollback
        // semantics. Internal cancel/pristine repair does not use this API.
        if self.goal_tracker.lock().snapshot().is_some() {
            return Ok(RewindResponse {
                success: false,
                target_prompt_index: request.target_prompt_index,
                mode: request.mode,
                reverted_files: vec![],
                clean_files: vec![],
                conflicts: vec![],
                prompt_text: None,
                error: Some(
                    "Cannot rewind while Goal state exists. Run /goal clear first.".to_string(),
                ),
            });
        }

        let target_index = request.target_prompt_index;
        let mode = request.mode;

        // Validate: target must be less than current prompt_index. FilesOnly
        // reverts the on-disk snapshot index (bounded by `get_rewind_points`,
        // not the conversation), so it is exempt — the chat-state prompt index
        // is empty in bridge mode, where the conversation lives server-side.
        let current_prompt_index = self.chat_state_handle.get_prompt_index().await;
        if mode != RewindMode::FilesOnly && target_index >= current_prompt_index {
            return Ok(RewindResponse {
                success: false,
                target_prompt_index: target_index,
                mode,
                reverted_files: vec![],
                clean_files: vec![],
                conflicts: vec![],
                prompt_text: None,
                error: Some(format!(
                    "Cannot rewind to prompt #{} — current prompt index is {}. \
                     Valid targets: 0..{}",
                    target_index,
                    current_prompt_index,
                    current_prompt_index.saturating_sub(1)
                )),
            });
        }

        // ── Build file revert preview (for All and FilesOnly modes) ─────
        let mut clean_files = Vec::new();
        let mut conflicts = Vec::new();

        let wants_file_revert = matches!(mode, RewindMode::All | RewindMode::FilesOnly);
        let wants_conversation_rewind =
            matches!(mode, RewindMode::All | RewindMode::ConversationOnly);

        // Collect files that would be reverted and detect conflicts.
        // This is read-only — no mutations happen here.
        let all_points = self.file_state_tracker.get_rewind_points().await;
        let mut files_to_revert: std::collections::BTreeMap<paths::RelPathBuf, Option<String>> =
            std::collections::BTreeMap::new();
        let mut current_files =
            std::collections::BTreeMap::<paths::RelPathBuf, Option<String>>::new();

        if wants_file_revert {
            for point in all_points.iter().filter(|p| p.prompt_index >= target_index) {
                for (path, before_snapshot) in &point.file_snapshots {
                    // Only keep the earliest snapshot for each file
                    files_to_revert
                        .entry(path.clone())
                        .or_insert_with(|| before_snapshot.content.clone());
                }
            }

            // Build conflict/clean lists for the preview
            for path in files_to_revert.keys() {
                let current_content = self
                    .tool_context
                    .fs
                    .try_read_to_string(path)
                    .await
                    .map_err(|error| {
                        anyhow::anyhow!("failed to read {} before rewind: {error}", path)
                    })?;
                current_files.insert(path.clone(), current_content.clone());

                // Find the latest after_snapshot for this file (what the agent
                // most recently left it as) for conflict detection.
                let after_content = all_points
                    .iter()
                    .rev()
                    .find_map(|p| p.after_snapshots.get(path))
                    .and_then(|s| s.content.clone());

                let is_clean = current_content == after_content;

                if is_clean {
                    clean_files.push(path.to_string());
                } else {
                    let conflict_type = if current_content.is_none() && after_content.is_some() {
                        "deleted_externally"
                    } else if current_content.is_some() && after_content.is_none() {
                        "created_externally"
                    } else {
                        "modified_externally"
                    };
                    conflicts.push(RewindConflictInfo {
                        path: path.to_string(),
                        conflict_type: conflict_type.to_string(),
                    });
                }
            }
        }

        // ── Preview mode (force=false): pure dry run, no mutations ────
        // Return what WOULD happen so the TUI can show a confirmation
        // modal. Nothing is written, deleted, or truncated.
        if !request.force {
            let error = if !conflicts.is_empty() {
                Some("External modifications detected. Confirm to revert anyway.".to_string())
            } else {
                None
            };
            return Ok(RewindResponse {
                success: false,
                target_prompt_index: target_index,
                mode,
                reverted_files: vec![],
                clean_files,
                conflicts,
                prompt_text: None,
                error,
            });
        }

        // ── Commit mode (force=true): execute the rewind ─────────────

        let transaction = crate::session::persistence::RewindTransaction {
            version: crate::session::persistence::REWIND_TRANSACTION_VERSION,
            target_prompt_index: target_index,
            pre_prompt_index: current_prompt_index,
            mode,
        };
        self.write_rewind_transaction(transaction).await?;

        let (reverted_files, changed_files) = if wants_file_revert {
            self.apply_file_rewind(&files_to_revert, &current_files)
                .await?
        } else {
            (Vec::new(), Vec::new())
        };

        let next_rewind_points = if wants_file_revert {
            all_points
                .iter()
                .filter(|point| point.prompt_index < target_index)
                .cloned()
                .collect()
        } else {
            workspace::session::file_state::merge_rewind_points_from(
                all_points.clone(),
                target_index,
            )
        };
        if let Err(error) = self.persist_rewind_points(next_rewind_points.clone()).await {
            let rollback = self
                .rollback_rewind_files(&changed_files, &current_files)
                .await;
            return Err(match rollback {
                Ok(()) => error,
                Err(rollback) => {
                    anyhow::anyhow!("{error}; file compensation also failed: {rollback}")
                }
            });
        }

        // Execute conversation rewind
        let mut prompt_text: Option<String> = None;
        if wants_conversation_rewind {
            if let Some(snap) = self.chat_state_handle.snapshot().await {
                prompt_text = snap
                    .prompt_records
                    .iter()
                    .find(|record| record.prompt_index == target_index)
                    .map(|record| record.text.clone());
            }

            // Timeline owns both branch selection and all derived prompt state.
            // There is no intermediate Chat snapshot to install.
            if let Err(error) = self.chat_state_handle.rewind_durably(target_index).await {
                let projection_rollback = self.persist_rewind_points(all_points.clone()).await;
                let file_rollback = self
                    .rollback_rewind_files(&changed_files, &current_files)
                    .await;
                let mut message = format!("failed to commit rewind Timeline: {error}");
                if let Err(rollback) = projection_rollback {
                    message.push_str(&format!("; rewind-point compensation failed: {rollback}"));
                }
                if let Err(rollback) = file_rollback {
                    message.push_str(&format!("; file compensation failed: {rollback}"));
                }
                anyhow::bail!(message);
            }

            // Store for edit-and-retry detection only after the branch fact is durable.
            if let Ok(mut pending) = self.rewind_pending_prompt.lock() {
                *pending = prompt_text.clone();
            }

            // Conversation shrank — clear budget-based (size/schema) and stale
            // per-turn suppression so compaction can run against the smaller context.
            // Provider-limit/auth suppression isn't budget-related, so it persists
            // until a successful model call.
            if self
                .compaction
                .auto_compact_suppressed
                .load(std::sync::atomic::Ordering::Relaxed)
                != crate::session::compaction_config::SUPPRESS_UNTIL_SUCCESS
            {
                self.compaction.auto_compact_suppressed.store(
                    crate::session::compaction_config::SUPPRESS_NONE,
                    std::sync::atomic::Ordering::Relaxed,
                );
            }

            // UI replay keeps a branch marker as a derived display cache.
            self.persist_update_only_durably(GrowSessionUpdate::RewindMarker {
                target_prompt_index: target_index,
                created_at: chrono::Utc::now().to_rfc3339(),
            })
            .await
            .map_err(|error| anyhow::anyhow!(
                "rewind Timeline committed, but its UI branch marker was not durably recorded: {error}"
            ))?;
        }

        self.file_state_tracker
            .replace_rewind_points(next_rewind_points)
            .await;
        self.clear_rewind_transaction().await?;
        self.signals_handle().mark_reverted();

        Ok(RewindResponse {
            success: true,
            target_prompt_index: target_index,
            mode,
            reverted_files,
            clean_files: vec![],
            conflicts,
            prompt_text,
            error: None,
        })
    }

    async fn persist_rewind_points(
        &self,
        points: Vec<workspace::session::file_state::RewindPoint>,
    ) -> anyhow::Result<()> {
        let (respond_to, response) = tokio::sync::oneshot::channel();
        self.notifications
            .persistence_tx
            .send(PersistenceMsg::ReplaceRewindPointsAndAck { points, respond_to })
            .map_err(|_| anyhow::anyhow!("rewind persistence actor is unavailable"))?;
        response
            .await
            .map_err(|_| anyhow::anyhow!("rewind persistence acknowledgement was dropped"))??;
        Ok(())
    }

    async fn write_rewind_transaction(
        &self,
        transaction: crate::session::persistence::RewindTransaction,
    ) -> anyhow::Result<()> {
        let (respond_to, response) = tokio::sync::oneshot::channel();
        self.notifications
            .persistence_tx
            .send(PersistenceMsg::WriteRewindTransactionAndAck {
                transaction,
                respond_to,
            })
            .map_err(|_| anyhow::anyhow!("rewind persistence actor is unavailable"))?;
        response
            .await
            .map_err(|_| anyhow::anyhow!("rewind transaction acknowledgement was dropped"))??;
        Ok(())
    }

    async fn clear_rewind_transaction(&self) -> anyhow::Result<()> {
        let (respond_to, response) = tokio::sync::oneshot::channel();
        self.notifications
            .persistence_tx
            .send(PersistenceMsg::ClearRewindTransactionAndAck { respond_to })
            .map_err(|_| anyhow::anyhow!("rewind persistence actor is unavailable"))?;
        response.await.map_err(|_| {
            anyhow::anyhow!("rewind transaction clear acknowledgement was dropped")
        })??;
        Ok(())
    }

    fn load_rewind_transaction(
        &self,
    ) -> anyhow::Result<Option<crate::session::persistence::RewindTransaction>> {
        let bytes = match self.session_directory.read_bounded(
            std::ffi::OsStr::new(crate::session::persistence::REWIND_TRANSACTION_FILE),
            "rewind transaction",
            crate::session::persistence::MAX_REWIND_TRANSACTION_BYTES,
        ) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let transaction =
            serde_json::from_slice::<crate::session::persistence::RewindTransaction>(&bytes)?;
        transaction.validate()?;
        Ok(Some(transaction))
    }

    /// Complete a forward-only rewind transaction left by process death.
    /// File and rewind-point projection steps precede the Timeline branch fact,
    /// so `current == target` proves the whole transaction committed and only
    /// the intent clear was lost. Otherwise the old typed points still contain
    /// everything needed to replay the idempotent forward steps.
    pub(super) async fn recover_pending_rewind(&self) -> anyhow::Result<()> {
        let Some(transaction) = self.load_rewind_transaction()? else {
            return Ok(());
        };
        let wants_files = matches!(transaction.mode, RewindMode::All | RewindMode::FilesOnly);
        let wants_conversation = matches!(
            transaction.mode,
            RewindMode::All | RewindMode::ConversationOnly
        );
        let current_prompt_index = self.chat_state_handle.get_prompt_index().await;
        if wants_conversation && current_prompt_index == transaction.target_prompt_index {
            self.file_state_tracker.get_rewind_points().await;
            self.clear_rewind_transaction().await?;
            return Ok(());
        }
        if wants_conversation && current_prompt_index != transaction.pre_prompt_index {
            anyhow::bail!(
                "pending rewind branch is neither source {} nor target {} (found {})",
                transaction.pre_prompt_index,
                transaction.target_prompt_index,
                current_prompt_index
            );
        }

        let all_points = self.file_state_tracker.get_rewind_points().await;
        let mut desired = std::collections::BTreeMap::<paths::RelPathBuf, Option<String>>::new();
        let mut originals = std::collections::BTreeMap::<paths::RelPathBuf, Option<String>>::new();
        if wants_files {
            for point in all_points
                .iter()
                .filter(|point| point.prompt_index >= transaction.target_prompt_index)
            {
                for (path, snapshot) in &point.file_snapshots {
                    desired
                        .entry(path.clone())
                        .or_insert_with(|| snapshot.content.clone());
                }
            }
            for path in desired.keys() {
                originals.insert(
                    path.clone(),
                    self.tool_context.fs.try_read_to_string(path).await?,
                );
            }
            self.apply_file_rewind(&desired, &originals).await?;
        }

        let next_points = if wants_files {
            all_points
                .iter()
                .filter(|point| point.prompt_index < transaction.target_prompt_index)
                .cloned()
                .collect()
        } else {
            workspace::session::file_state::merge_rewind_points_from(
                all_points,
                transaction.target_prompt_index,
            )
        };
        self.persist_rewind_points(next_points.clone()).await?;
        if wants_conversation {
            self.chat_state_handle
                .rewind_durably(transaction.target_prompt_index)
                .await
                .map_err(|error| anyhow::anyhow!("failed to recover rewind Timeline: {error}"))?;
        }
        self.file_state_tracker
            .replace_rewind_points(next_points)
            .await;
        self.clear_rewind_transaction().await?;
        self.signals_handle().mark_reverted();
        Ok(())
    }

    async fn apply_file_rewind(
        &self,
        desired: &std::collections::BTreeMap<paths::RelPathBuf, Option<String>>,
        originals: &std::collections::BTreeMap<paths::RelPathBuf, Option<String>>,
    ) -> anyhow::Result<(Vec<String>, Vec<paths::RelPathBuf>)> {
        let mut reverted = Vec::with_capacity(desired.len());
        let mut changed = Vec::new();
        for (path, content) in desired {
            let original = originals
                .get(path)
                .ok_or_else(|| anyhow::anyhow!("rewind preview omitted {}", path))?;
            reverted.push(path.to_string());
            if original == content {
                continue;
            }
            let result = match content {
                Some(data) => self.tool_context.fs.write_file(path, data.as_bytes()).await,
                None => self.tool_context.fs.delete_file(path).await,
            };
            if let Err(error) = result {
                let rollback = self.rollback_rewind_files(&changed, originals).await;
                return Err(match rollback {
                    Ok(()) => anyhow::anyhow!("failed to restore {} during rewind: {error}", path),
                    Err(rollback) => anyhow::anyhow!(
                        "failed to restore {} during rewind: {error}; compensation failed: {rollback}",
                        path
                    ),
                });
            }
            changed.push(path.clone());
        }
        Ok((reverted, changed))
    }

    async fn rollback_rewind_files(
        &self,
        changed: &[paths::RelPathBuf],
        originals: &std::collections::BTreeMap<paths::RelPathBuf, Option<String>>,
    ) -> anyhow::Result<()> {
        let mut failures = Vec::new();
        for path in changed.iter().rev() {
            let Some(original) = originals.get(path) else {
                failures.push(format!("{} has no captured original", path));
                continue;
            };
            let result = match original {
                Some(data) => self.tool_context.fs.write_file(path, data.as_bytes()).await,
                None => self.tool_context.fs.delete_file(path).await,
            };
            if let Err(error) = result {
                failures.push(format!("{}: {error}", path));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(failures.join(", "))
        }
    }

    /// Out-of-band history repair (`grow/session/repair`) for a resident
    /// session: run the repair inside the chat-state actor and acknowledge
    /// only after its canonical Timeline replacement is durable.
    ///
    /// Refused while a turn is in flight (in-flight tool calls legitimately
    /// await their results). The refusal is enforced inside the chat-state
    /// actor's command handler — see `ChatStateCommand::RepairHistory` for
    /// why a caller-side check alone would race turn start; the check below
    /// is just a fast path.
    pub(super) async fn handle_repair_history(
        &self,
        dry_run: bool,
    ) -> anyhow::Result<chat_state::compaction_utils::HistoryRepairReport> {
        // Per-session flag — NOT `tool_context.is_turn_active`, which is the
        // agent-wide coordinator flag shared by all sessions (using it would
        // refuse repair of an idle session while any other session runs a
        // turn, and another session's turn end could clear it mid-turn).
        let turn_flag = self.session_turn_active.clone();
        if turn_flag.load(std::sync::atomic::Ordering::SeqCst) {
            anyhow::bail!(chat_state::RepairHistoryError::TurnActive);
        }

        let report = self
            .chat_state_handle
            .repair_history(dry_run, Some(turn_flag))
            .await
            .ok_or_else(|| anyhow::anyhow!("chat-state actor unavailable"))?
            .map_err(anyhow::Error::new)?;

        if report.changed() && !dry_run {
            tracing::warn!(
                session_id = %self.session_info.id.0,
                duplicates_removed = report.duplicates_removed,
                stripped_tool_result_ids = ?report.stripped_tool_result_ids,
                synthetic_results_inserted = report.synthetic_results_inserted,
                "session history repaired"
            );
        }

        Ok(report)
    }
}
