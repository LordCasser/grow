use super::*;

pub(super) struct QueuedCoordinationInquiry {
    inquiry: crate::coordination::InboundInquiry,
    _activity: super::tasks_cancel::SessionActivityPermit,
}

impl SessionActor {
    pub(super) async fn enqueue_coordination_inquiry(
        self: &Arc<Self>,
        inquiry: crate::coordination::InboundInquiry,
    ) {
        match self.existing_coordination_outcome(&inquiry).await {
            Ok(Some(outcome)) => {
                if let Err(error) = self.reconcile_coordination_answers().await {
                    tracing::warn!(%error, "existing consultation awaits context synchronization");
                }
                inquiry
                    .progress
                    .send_replace(crate::coordination::InquiryPhase::Finished);
                let _ = inquiry.respond_to.send(outcome);
                return;
            }
            Err(error) => {
                reject_inquiry(inquiry, crate::coordination::InquiryStatus::Failed, error);
                return;
            }
            Ok(None) => {}
        }
        // Receipt is durable before queued/approval/running is observable.
        if let Err(error) = self.record_incoming_coordination_notice(&inquiry).await {
            reject_inquiry(
                inquiry,
                crate::coordination::InquiryStatus::Failed,
                crate::coordination::CoordinationError::new(
                    crate::coordination::CoordinationErrorCode::AuditFailure,
                    error,
                ),
            );
            return;
        }
        if inquiry.cancellation.is_cancelled() {
            self.complete_rejected_coordination_inquiry_with_audit(
                inquiry,
                crate::coordination::InquiryStatus::Cancelled,
                "coordination inquiry was cancelled before admission".into(),
            );
            return;
        }
        let Some(activity) = self.session_activities.try_start("coordination_inquiry") else {
            self.complete_rejected_coordination_inquiry_with_audit(
                inquiry,
                crate::coordination::InquiryStatus::Unavailable,
                crate::coordination::CoordinationError::new(
                    crate::coordination::CoordinationErrorCode::NotFound,
                    "target session is shutting down",
                ),
            );
            return;
        };
        let mut queue = self.coordination_inquiries.borrow_mut();
        if self.coordination_inquiry_active.get()
            && queue.len() >= crate::coordination::MAX_QUEUED_INQUIRIES
        {
            drop(queue);
            drop(activity);
            self.complete_rejected_coordination_inquiry_with_audit(
                inquiry,
                crate::coordination::InquiryStatus::Unavailable,
                crate::coordination::CoordinationError::retry(
                    crate::coordination::CoordinationErrorCode::Busy,
                    "target session inquiry queue is full",
                    1000,
                ),
            );
            return;
        }
        inquiry
            .progress
            .send_replace(crate::coordination::InquiryPhase::Queued);
        queue.push_back(QueuedCoordinationInquiry {
            inquiry,
            _activity: activity,
        });
        if self.coordination_inquiry_active.replace(true) {
            return;
        }
        drop(queue);

        let session = Arc::clone(self);
        tokio::task::spawn_local(async move {
            session.run_coordination_inquiry_queue().await;
        });
    }

    async fn existing_coordination_outcome(
        &self,
        inquiry: &crate::coordination::InboundInquiry,
    ) -> Result<Option<crate::coordination::InquiryOutcome>, crate::coordination::CoordinationError>
    {
        use crate::coordination::{CoordinationError, CoordinationErrorCode, InquiryEvent};
        let events = self
            .chat_state_handle
            .timeline_events()
            .await
            .ok_or_else(|| {
                CoordinationError::new(
                    CoordinationErrorCode::AuditFailure,
                    "Timeline actor unavailable",
                )
            })?;
        let mut received = false;
        for event in &events {
            let Some(InquiryEvent::Incoming { inquiry_id, audit }) =
                InquiryEvent::from_timeline(event).map_err(|error| {
                    CoordinationError::new(CoordinationErrorCode::AuditFailure, error)
                })?
            else {
                continue;
            };
            if inquiry_id != inquiry.inquiry_id
                || audit.source_session_id != inquiry.source_session_id
            {
                continue;
            }
            if audit.question != inquiry.question
                || audit.source_cwd != inquiry.source_cwd
                || audit.direction != inquiry.direction
                || audit.delegated_subagent_task_name != inquiry.delegated_subagent_task_name
            {
                return Err(CoordinationError::new(
                    CoordinationErrorCode::Conflict,
                    "inquiry identity already belongs to a different received request",
                ));
            }
            received = true;
            if let Some(outcome) = audit.outcome {
                return Ok(Some(outcome));
            }
        }
        if received {
            return Err(CoordinationError::retry(
                CoordinationErrorCode::Busy,
                "inquiry has an open durable receipt and will not be executed twice",
                1000,
            ));
        }
        Ok(None)
    }

    async fn run_coordination_inquiry_queue(self: Arc<Self>) {
        loop {
            let queued = self.coordination_inquiries.borrow_mut().pop_front();
            let Some(queued) = queued else {
                self.coordination_inquiry_active.set(false);
                return;
            };
            let inquiry = queued.inquiry;
            let mut answer_context = None;
            let outcome = if inquiry.cancellation.is_cancelled() {
                inquiry.cancellation.outcome(&inquiry.inquiry_id)
            } else {
                self.handle_coordination_inquiry(&inquiry, &mut answer_context)
                    .await
            };
            let outcome = match self
                .record_coordination_terminal_notice(&inquiry, &outcome, answer_context)
                .await
            {
                Ok(outcome) => outcome,
                Err(error) => failed(
                    &inquiry,
                    crate::coordination::CoordinationError::new(
                        crate::coordination::CoordinationErrorCode::AuditFailure,
                        format!("failed to persist target coordination audit: {error}"),
                    ),
                ),
            };
            // Consultation success is independent of later context synchronization.
            // A failed synchronization remains a canonical obligation for the next boundary.
            if let Err(error) = self.reconcile_coordination_answers().await {
                tracing::warn!(%error, "consultation answer awaits context synchronization");
            }
            let _ = inquiry.respond_to.send(outcome);
        }
    }

    async fn handle_coordination_inquiry(
        &self,
        inquiry: &crate::coordination::InboundInquiry,
        answer_context: &mut Option<crate::coordination::InquiryAnswerContextProof>,
    ) -> crate::coordination::InquiryOutcome {
        let delegated = inquiry.authority == crate::coordination::InquiryAuthority::Delegation;
        let same_cwd = match (
            dunce::canonicalize(&inquiry.source_cwd),
            dunce::canonicalize(&self.session_info.cwd),
        ) {
            (Ok(source), Ok(target)) => source == target,
            _ => false,
        };
        if !same_cwd && !delegated {
            let target_cwd = crate::coordination::canonical_cwd(Path::new(&self.session_info.cwd));
            inquiry
                .progress
                .send_replace(crate::coordination::InquiryPhase::AwaitingApproval);
            match self
                .request_coordination_approval(inquiry, &target_cwd)
                .await
            {
                CoordinationApproval::Allowed => {
                    if let Err(error) = self
                        .record_coordination_approval_notice(inquiry, "approved")
                        .await
                    {
                        return failed(
                            inquiry,
                            format!("failed to persist coordination approval audit: {error}"),
                        );
                    }
                }
                CoordinationApproval::Rejected => {
                    if let Err(error) = self
                        .record_coordination_approval_notice(inquiry, "rejected")
                        .await
                    {
                        return failed(
                            inquiry,
                            format!("failed to persist coordination rejection audit: {error}"),
                        );
                    }
                    return crate::coordination::InquiryOutcome::terminal(
                        &inquiry.inquiry_id,
                        crate::coordination::InquiryStatus::Rejected,
                        "target user rejected the cross-workspace inquiry",
                    );
                }
                CoordinationApproval::Cancelled => {
                    if let Err(error) = self
                        .record_coordination_approval_notice(inquiry, "cancelled")
                        .await
                    {
                        return failed(
                            inquiry,
                            format!("failed to persist coordination cancellation audit: {error}"),
                        );
                    }
                    return inquiry.cancellation.outcome(&inquiry.inquiry_id);
                }
                CoordinationApproval::TimedOut => {
                    if let Err(error) = self
                        .record_coordination_approval_notice(inquiry, "timed out")
                        .await
                    {
                        return failed(
                            inquiry,
                            format!("failed to persist coordination timeout audit: {error}"),
                        );
                    }
                    return crate::coordination::InquiryOutcome::terminal(
                        &inquiry.inquiry_id,
                        crate::coordination::InquiryStatus::TimedOut,
                        "cross-workspace inquiry approval timed out",
                    );
                }
                CoordinationApproval::Unavailable => {
                    if let Err(error) = self
                        .record_coordination_approval_notice(
                            inquiry,
                            "rejected because no online UI is available",
                        )
                        .await
                    {
                        return failed(
                            inquiry,
                            format!("failed to persist coordination rejection audit: {error}"),
                        );
                    }
                    return crate::coordination::InquiryOutcome::terminal(
                        &inquiry.inquiry_id,
                        crate::coordination::InquiryStatus::Rejected,
                        "target session has no online UI for approval",
                    );
                }
            }
        }
        if (same_cwd || delegated)
            && let Err(error) = self
                .record_coordination_approval_notice(
                    inquiry,
                    if delegated {
                        "approved (direct delegation)"
                    } else {
                        "approved (same workspace)"
                    },
                )
                .await
        {
            return failed(
                inquiry,
                format!("failed to persist coordination approval audit: {error}"),
            );
        }
        if inquiry.cancellation.is_cancelled() {
            return inquiry.cancellation.outcome(&inquiry.inquiry_id);
        }
        inquiry
            .progress
            .send_replace(crate::coordination::InquiryPhase::Running);

        let prepared = self.prepare_chat_completion_config(false).await;
        let model = prepared.model.clone();
        let context_window = prepared.context_window;
        let image_key = sampling_types::model_image_input_key_from_parts(
            &prepared.model,
            &prepared.api_backend,
            &prepared.base_url,
            &prepared.query_params,
        );
        let sampling_client = match sampler::SamplingClient::new(prepared) {
            Ok(client) => client,
            Err(error) => return failed(inquiry, format!("failed to prepare model: {error}")),
        };
        let context = tokio::select! {
            biased;
            _ = inquiry.cancellation.cancelled() => {
                return inquiry.cancellation.outcome(&inquiry.inquiry_id);
            }
            _ = self.sideband_cancel.cancelled() => {
                return crate::coordination::InquiryOutcome::terminal(
                    &inquiry.inquiry_id,
                    crate::coordination::InquiryStatus::Cancelled,
                    "target session closed during coordination inquiry",
                );
            }
            context = self.freeze_sideband_surface() => context,
        };
        let materialized = match context {
            Ok(context) => context,
            Err(error) => {
                return failed(
                    inquiry,
                    format!("coordination context unavailable: {error}"),
                );
            }
        };
        let frozen = (
            &materialized.surface[..],
            &materialized.surface_ids[..],
            materialized.surface_revision,
        );
        let mut items = sampling_types::project_portable_history(&materialized.surface);
        let prepared_images = tokio::select! {
            biased;
            _ = inquiry.cancellation.cancelled() => return inquiry.cancellation.outcome(&inquiry.inquiry_id),
            result = self.prepare_sideband_images(&mut items, frozen, &image_key) => result,
        };
        if let Err(error) = prepared_images {
            return failed(inquiry, error);
        }
        let prompt = crate::coordination::inquiry_prompt(
            &inquiry.source_session_id,
            &inquiry.source_cwd,
            &inquiry.question,
        );
        items.push(ConversationItem::user(prompt.clone()));
        let mut request = ConversationRequest {
            items,
            model: Some(model.clone()),
            temperature: None,
            ..Default::default()
        };
        debug_assert!(request.tools.is_empty());
        debug_assert!(request.tool_choice.is_none());
        let mut sideband = match self
            .begin_sideband(
                chat_state::SidebandPurpose::InfoRequest,
                prompt,
                SidebandSource::Frozen(vec![materialized.input_ref.clone()]),
                {
                    let mut policy = chat_state::SidebandBudgetPolicy::for_request(&request, 2);
                    policy.max_input_tokens_per_attempt = context_window;
                    policy
                },
                chat_state::SidebandRoute {
                    model,
                    backend: sampling_client.api_backend(),
                },
                None,
            )
            .await
        {
            Ok(sideband) => sideband,
            Err(error) => return failed(inquiry, format!("failed to start sideband: {error}")),
        };
        if let Err(error) = sideband
            .attempt_all_sources(&request, sampling_client.api_backend(), None)
            .await
        {
            return failed(
                inquiry,
                format!("failed to persist sideband attempt: {error}"),
            );
        }
        let provider = self.collect_sideband_with_image_recovery(
            &mut sideband,
            &sampling_client,
            &mut request,
            frozen,
            &image_key,
        );
        let response = tokio::select! {
            biased;
            _ = inquiry.cancellation.cancelled() => None,
            response = provider => Some(response),
        };
        let Some(response) = response else {
            let _ = self.settle_sideband_attempt_incomplete(&mut sideband).await;
            let _ = sideband
                .fail(
                    chat_state::SidebandOutcome::Cancelled,
                    "coordination inquiry cancelled",
                )
                .await;
            return inquiry.cancellation.outcome(&inquiry.inquiry_id);
        };
        let response = match response {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                let _ = self.settle_sideband_attempt_incomplete(&mut sideband).await;
                let _ = sideband
                    .fail(chat_state::SidebandOutcome::Failed, error.to_string())
                    .await;
                return failed(inquiry, format!("coordination model call failed: {error}"));
            }
            Err(SidebandRunError::Cancelled) => {
                return crate::coordination::InquiryOutcome::terminal(
                    &inquiry.inquiry_id,
                    crate::coordination::InquiryStatus::Cancelled,
                    "target session closed during coordination inquiry",
                );
            }
            Err(error) => {
                return failed(inquiry, format!("coordination sideband failed: {error}"));
            }
        };
        let usage = match self
            .settle_sideband_response_usage(&mut sideband, &response)
            .await
        {
            Ok(usage) => usage,
            Err(error) => return failed(inquiry, format!("failed to settle model usage: {error}")),
        };
        let answer = response.assistant_text();
        if answer.trim().is_empty() {
            let _ = sideband
                .fail(
                    chat_state::SidebandOutcome::Failed,
                    "coordination model returned an empty response",
                )
                .await;
            return failed(inquiry, "coordination model returned an empty response");
        }
        let finish = sideband_finish(&response);
        let result_ref = match sideband
            .complete(answer.clone(), None, usage, finish, Vec::new())
            .await
        {
            Ok(result_ref) => result_ref,
            Err(error) => {
                return failed(
                    inquiry,
                    format!("failed to persist coordination answer: {error}"),
                );
            }
        };
        let proof = crate::coordination::InquiryAnswerContextProof {
            target_session_id: self.session_info.id.to_string(),
            result_ref,
        };
        let mut audit = crate::coordination::IncomingInquiryAudit::received(inquiry);
        audit.outcome = Some(crate::coordination::InquiryOutcome::answered(
            &inquiry.inquiry_id,
            answer,
        ));
        audit.answer_context = Some(proof.clone());
        let body = audit
            .answer_context_body()
            .expect("completed consultation has an answer and proof");
        if body.len() as u64 > chat_state::MAX_NOTIFICATION_PAYLOAD_BYTES {
            return failed(
                inquiry,
                format!(
                    "consultation context exceeds the {} byte notification payload limit",
                    chat_state::MAX_NOTIFICATION_PAYLOAD_BYTES
                ),
            );
        }
        if inquiry.authority == crate::coordination::InquiryAuthority::Peer
            && let Err(error) = crate::coordination::validate_peer_inquiry_outcome(
                audit
                    .outcome
                    .as_ref()
                    .expect("completed consultation has an outcome"),
            )
        {
            return failed(
                inquiry,
                format!("consultation result exceeds its IPC frame: {error}"),
            );
        }
        *answer_context = Some(proof);
        audit
            .outcome
            .expect("completed consultation has an outcome")
    }

    fn complete_rejected_coordination_inquiry_with_audit(
        self: &Arc<Self>,
        inquiry: crate::coordination::InboundInquiry,
        status: crate::coordination::InquiryStatus,
        error: crate::coordination::CoordinationError,
    ) {
        let session = Arc::clone(self);
        tokio::task::spawn_local(async move {
            let mut outcome =
                crate::coordination::InquiryOutcome::terminal(&inquiry.inquiry_id, status, error);
            if let Err(audit_error) = session
                .record_coordination_terminal_notice(&inquiry, &outcome, None)
                .await
            {
                outcome = failed(
                    &inquiry,
                    format!("failed to persist target coordination audit: {audit_error}"),
                );
            }
            let _ = inquiry.respond_to.send(outcome);
        });
    }

    pub(super) async fn persist_coordination_inquiry(
        &self,
        event: crate::coordination::InquiryEvent,
    ) -> Result<(), crate::coordination::CoordinationError> {
        self.persist_coordination_inquiry_with_cancellation(event, None)
            .await
            .map(|_| ())
    }

    async fn persist_coordination_inquiry_with_cancellation(
        &self,
        mut event: crate::coordination::InquiryEvent,
        cancellation: Option<&crate::coordination::InquiryCancellation>,
    ) -> Result<crate::coordination::InquiryEvent, crate::coordination::CoordinationError> {
        let answer_gate = if matches!(&event, crate::coordination::InquiryEvent::Incoming { audit, .. } if audit.answer_context.is_some())
        {
            Some(self.step_control_gate.lock().await)
        } else {
            None
        };
        if let crate::coordination::InquiryEvent::OutgoingStarted {
            inquiry_id,
            target_session_id,
            question,
        } = &event
        {
            let events = self
                .chat_state_handle
                .timeline_events()
                .await
                .ok_or_else(|| {
                    crate::coordination::CoordinationError::new(
                        crate::coordination::CoordinationErrorCode::AuditFailure,
                        "Timeline actor unavailable while admitting source inquiry",
                    )
                })?;
            for timeline_event in &events {
                let fact = crate::coordination::InquiryEvent::from_timeline(timeline_event)
                    .map_err(|error| {
                        crate::coordination::CoordinationError::new(
                            crate::coordination::CoordinationErrorCode::AuditFailure,
                            format!("could not inspect source inquiry Timeline: {error}"),
                        )
                    })?;
                let Some(fact) = fact else {
                    continue;
                };
                let (existing_id, existing_target, existing_question) = match &fact {
                    crate::coordination::InquiryEvent::OutgoingStarted {
                        inquiry_id,
                        target_session_id,
                        question,
                    } => (inquiry_id, target_session_id, question),
                    crate::coordination::InquiryEvent::OutgoingCompleted { audit } => (
                        &audit.outcome.inquiry_id,
                        &audit.target_session_id,
                        &audit.question,
                    ),
                    crate::coordination::InquiryEvent::Incoming { .. } => continue,
                };
                if existing_id != inquiry_id {
                    continue;
                }
                if existing_target == target_session_id && existing_question == question {
                    return Ok(event);
                }
                return Err(crate::coordination::CoordinationError::new(
                    crate::coordination::CoordinationErrorCode::Conflict,
                    "inquiry id already belongs to a different durable request",
                ));
            }
        }
        if let crate::coordination::InquiryEvent::Incoming { inquiry_id, audit } = &event
            && audit.answer_context.is_some()
        {
            let events = self
                .chat_state_handle
                .timeline_events()
                .await
                .ok_or_else(|| {
                    crate::coordination::CoordinationError::new(
                        crate::coordination::CoordinationErrorCode::AuditFailure,
                        "Timeline unavailable while committing consultation answer",
                    )
                })?;
            for previous in &events {
                let Some(crate::coordination::InquiryEvent::Incoming {
                    inquiry_id: previous_id,
                    audit: previous,
                }) = crate::coordination::InquiryEvent::from_timeline(previous)
                    .map_err(crate::coordination::CoordinationError::from)?
                else {
                    continue;
                };
                if previous_id == *inquiry_id
                    && previous.source_session_id == audit.source_session_id
                    && previous.outcome.is_some()
                {
                    if serde_json::to_value(&previous).unwrap()
                        == serde_json::to_value(audit).unwrap()
                    {
                        return Ok(event);
                    }
                    return Err(crate::coordination::CoordinationError::new(
                        crate::coordination::CoordinationErrorCode::Conflict,
                        "inquiry already has a different canonical terminal",
                    ));
                }
            }
        }
        // The existing answer fence has resolved all prior canonical facts.
        // Seal only now: cancellation can win across every preparation await,
        // but cannot reverse this append once success admission has won.
        if let Some(cancellation) = cancellation
            && let crate::coordination::InquiryEvent::Incoming { inquiry_id, audit } = &mut event
            && audit.answer_context.is_some()
            && !cancellation.admit_success()
        {
            audit.outcome = Some(cancellation.outcome(inquiry_id));
            audit.answer_context = None;
        }
        self.chat_state_handle
            .record_timeline_event_durably(event.timeline_kind())
            .await
            .map_err(|error| {
                crate::coordination::CoordinationError::new(
                    crate::coordination::CoordinationErrorCode::AuditFailure,
                    error.to_string(),
                )
            })?;
        drop(answer_gate);
        let notice = event.notice();
        let projection = if matches!(event, crate::coordination::InquiryEvent::Incoming { .. }) {
            self.persist_ui_notice(notice).await
        } else {
            self.persist_grow_audit_notification(GrowSessionUpdate::UiNotice(notice))
                .await
                .map_err(|error| error.to_string())
        };
        if let Err(error) = projection {
            tracing::warn!(%error, "failed to project durable inquiry into UI replay");
        }
        Ok(event)
    }

    async fn record_incoming_coordination_notice(
        &self,
        inquiry: &crate::coordination::InboundInquiry,
    ) -> Result<(), String> {
        self.persist_coordination_inquiry(crate::coordination::InquiryEvent::Incoming {
            inquiry_id: inquiry.inquiry_id.clone(),
            audit: crate::coordination::IncomingInquiryAudit::received(inquiry),
        })
        .await
        .map_err(|error| error.to_string())
    }

    async fn record_coordination_approval_notice(
        &self,
        inquiry: &crate::coordination::InboundInquiry,
        result: &str,
    ) -> Result<(), String> {
        let mut audit = crate::coordination::IncomingInquiryAudit::received(inquiry);
        audit.approval = Some(result.into());
        self.persist_coordination_inquiry(crate::coordination::InquiryEvent::Incoming {
            inquiry_id: inquiry.inquiry_id.clone(),
            audit,
        })
        .await
        .map_err(|error| error.to_string())
    }

    async fn record_coordination_terminal_notice(
        &self,
        inquiry: &crate::coordination::InboundInquiry,
        outcome: &crate::coordination::InquiryOutcome,
        answer_context: Option<crate::coordination::InquiryAnswerContextProof>,
    ) -> Result<crate::coordination::InquiryOutcome, String> {
        let mut audit = crate::coordination::IncomingInquiryAudit::received(inquiry);
        audit.outcome = Some(outcome.clone());
        audit.answer_context = answer_context;
        let terminal = self
            .persist_coordination_inquiry_with_cancellation(
                crate::coordination::InquiryEvent::Incoming {
                    inquiry_id: inquiry.inquiry_id.clone(),
                    audit,
                },
                Some(&inquiry.cancellation),
            )
            .await
            .map_err(|error| error.to_string())?;
        let crate::coordination::InquiryEvent::Incoming { audit, .. } = terminal else {
            unreachable!("incoming terminal returns its canonical event");
        };
        Ok(audit.outcome.expect("terminal notice has an outcome"))
    }

    /// Coordinate a captured canonical frontier, not a second delivery ledger.
    /// Concurrent calls can replay a suffix, but advance only after every obligation
    /// through their own captured end is backed by an idempotent durable receipt.
    pub(super) async fn reconcile_coordination_answers(&self) -> Result<(), String> {
        let events = self
            .chat_state_handle
            .timeline_events_from(self.coordination_answer_next_seq.get())
            .await
            .ok_or("Timeline actor unavailable during consultation synchronization")?;
        let next_seq = events.last().map(|event| event.seq.get() + 1);
        for event in &events {
            let Some(crate::coordination::InquiryEvent::Incoming { inquiry_id, audit }) =
                crate::coordination::InquiryEvent::from_timeline(event)?
            else {
                continue;
            };
            let Some(proof) = &audit.answer_context else {
                continue;
            };
            // Inherited observations are history, never local execution authority.
            if proof.target_session_id != self.session_info.id.to_string() {
                continue;
            }
            let source = chat_state::NotificationSource::InquiryAnswered {
                source_session_id: audit.source_session_id.clone(),
                inquiry_id,
            };
            let version = chat_state::NotificationSourceVersion::Opaque {
                value: format!("{}:{}", self.session_info.id, event.seq.get()),
            };
            if self
                .chat_state_handle
                .received_notification_id(source.clone(), version.clone())
                .await
                .ok_or("Timeline actor unavailable during receipt lookup")?
                .is_some()
            {
                continue;
            }
            let body = audit
                .answer_context_body()
                .ok_or("consultation proof has no answer")?;
            let _control_gate = self.step_control_gate.lock().await;
            if !self.state.lock().await.execution_open() {
                // The canonical obligation survives suspension/termination;
                // only an open original writer may admit its derived receipt.
                return Ok(());
            }
            self.receive_notification_detailed(source, version, body, None)
                .await
                .map_err(|error| error.to_string())?;
        }
        if let Some(next_seq) = next_seq {
            self.coordination_answer_next_seq
                .set(self.coordination_answer_next_seq.get().max(next_seq));
        }
        Ok(())
    }

    /// The actor already owns the validated Timeline, including restore facts.
    async fn pending_coordination_notices(
        &self,
    ) -> Result<Vec<(String, crate::coordination::IncomingInquiryAudit)>, String> {
        let events = self
            .chat_state_handle
            .timeline_events()
            .await
            .ok_or("Timeline actor unavailable")?;
        let mut latest =
            indexmap::IndexMap::<(String, String), crate::coordination::IncomingInquiryAudit>::new(
            );
        for event in &events {
            let Some(crate::coordination::InquiryEvent::Incoming { inquiry_id, audit }) =
                crate::coordination::InquiryEvent::from_timeline(event)?
            else {
                continue;
            };
            let key = (audit.source_session_id.clone(), inquiry_id);
            if latest
                .get(&key)
                .is_none_or(|previous| previous.outcome.is_none())
            {
                latest.insert(key, audit);
            }
        }
        Ok(latest
            .into_iter()
            .filter_map(|((_, id), audit)| audit.outcome.is_none().then_some((id, audit)))
            .collect())
    }

    /// Only called while constructing a new SessionActor, under its exclusive
    /// writer epoch. No process-local inquiry can have survived that boundary.
    pub(super) async fn recover_interrupted_coordination_inquiries(&self) -> Result<(), String> {
        for (id, mut audit) in self.pending_coordination_notices().await? {
            audit.outcome = Some(crate::coordination::InquiryOutcome::terminal(
                &id,
                crate::coordination::InquiryStatus::Unavailable,
                crate::coordination::CoordinationError::new(
                    crate::coordination::CoordinationErrorCode::TargetRestarted,
                    "target session restarted before a terminal answer was recorded; the inquiry was not replayed",
                ),
            ));
            self.persist_coordination_inquiry(crate::coordination::InquiryEvent::Incoming {
                inquiry_id: id,
                audit,
            })
            .await
            .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    /// Reconnect to a resident actor republishes its open receipt as live,
    /// without another durable event, model request, or notification hook.
    pub(super) async fn publish_coordination_state(&self) -> Result<(), String> {
        for (id, audit) in self.pending_coordination_notices().await? {
            self.send_transient_passive_notification(GrowSessionUpdate::UiNotice(
                audit.notice(&id),
            ));
        }
        Ok(())
    }

    async fn request_coordination_approval(
        &self,
        inquiry: &crate::coordination::InboundInquiry,
        target_cwd: &str,
    ) -> CoordinationApproval {
        use std::sync::atomic::Ordering;

        if !self.notifications.gateway_enabled.load(Ordering::Acquire) {
            return CoordinationApproval::Unavailable;
        }
        let tool_call_id = acp::ToolCallId::new(inquiry.inquiry_id.clone());
        let tool_call = acp::ToolCallUpdate::new(
            tool_call_id.clone(),
            acp::ToolCallUpdateFields::new()
                .title(Some(format!(
                    "Allow inquiry from session {}",
                    inquiry.source_session_id
                )))
                .kind(Some(acp::ToolKind::Other))
                .content(Some(vec![acp::ToolCallContent::from(
                    acp::ContentBlock::Text(acp::TextContent::new(inquiry.question.clone())),
                )]))
                .raw_input(Some(serde_json::json!({
                    "sourceSessionId": inquiry.source_session_id,
                    "sourceCwd": inquiry.source_cwd,
                    "targetCwd": target_cwd,
                    "question": inquiry.question,
                }))),
        );
        let request = acp::RequestPermissionRequest::new(
            self.session_info.id.clone(),
            tool_call,
            vec![
                acp::PermissionOption::new(
                    "allow-once",
                    "Allow this inquiry",
                    acp::PermissionOptionKind::AllowOnce,
                ),
                acp::PermissionOption::new(
                    "reject-once",
                    "Reject",
                    acp::PermissionOptionKind::RejectOnce,
                ),
            ],
        );
        let _pending = crate::session::pending_interaction::PendingInteractionGuard::new(
            self.pending_interactions.clone(),
            self.notifications.gateway.clone(),
            self.session_info.id.clone(),
            inquiry.inquiry_id.clone(),
            crate::session::pending_interaction::PendingKind::Permission,
        );
        let response = tokio::select! {
            biased;
            _ = inquiry.cancellation.cancelled() => return CoordinationApproval::Cancelled,
            response = tokio::time::timeout(
                crate::coordination::APPROVAL_TIMEOUT,
                self.notifications.gateway.send(request),
            ) => response,
        };
        match response {
            Err(_) => CoordinationApproval::TimedOut,
            Ok(Err(_)) => CoordinationApproval::Unavailable,
            Ok(Ok(response)) => match response.outcome {
                acp::RequestPermissionOutcome::Selected(selected)
                    if selected.option_id.0.as_ref() == "allow-once" =>
                {
                    CoordinationApproval::Allowed
                }
                acp::RequestPermissionOutcome::Selected(_) => CoordinationApproval::Rejected,
                acp::RequestPermissionOutcome::Cancelled => CoordinationApproval::Cancelled,
                _ => CoordinationApproval::Rejected,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoordinationApproval {
    Allowed,
    Rejected,
    Cancelled,
    TimedOut,
    Unavailable,
}

fn reject_inquiry(
    inquiry: crate::coordination::InboundInquiry,
    status: crate::coordination::InquiryStatus,
    error: impl Into<crate::coordination::CoordinationError>,
) {
    let _ = inquiry
        .respond_to
        .send(crate::coordination::InquiryOutcome::terminal(
            inquiry.inquiry_id,
            status,
            error,
        ));
}

fn failed(
    inquiry: &crate::coordination::InboundInquiry,
    error: impl Into<crate::coordination::CoordinationError>,
) -> crate::coordination::InquiryOutcome {
    crate::coordination::InquiryOutcome::terminal(
        &inquiry.inquiry_id,
        crate::coordination::InquiryStatus::Failed,
        error,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    mod identity_tests;

    struct JsonlTimelinePersistence {
        storage: crate::session::storage::JsonlStorageAdapter,
        info: crate::session::info::Info,
    }

    impl chat_state::TimelinePersistence for JsonlTimelinePersistence {
        fn persist_timeline_event_and_ack(
            &mut self,
            event: &chat_state::TimelineEvent,
        ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
            use crate::session::storage::StorageAdapter as _;

            let storage = self.storage.clone();
            let info = self.info.clone();
            let event = event.clone();
            let (respond_to, receiver) = tokio::sync::oneshot::channel();
            tokio::spawn(async move {
                let _ = respond_to.send(storage.append_timeline_event_durable(&info, &event).await);
            });
            receiver
        }

        fn flush(&mut self) {}
    }

    async fn admit_and_ack_startup(
        actor: &Arc<SessionActor>,
        event: crate::coordination::InquiryEvent,
        updates: &mut tokio::sync::mpsc::UnboundedReceiver<
            crate::session::persistence::PersistenceMsg,
        >,
    ) -> Result<(), crate::coordination::CoordinationError> {
        let task_actor = Arc::clone(actor);
        let task = tokio::task::spawn_local(async move {
            task_actor.persist_coordination_inquiry(event).await
        });
        let Some(crate::session::persistence::PersistenceMsg::AppendUpdateDurablyAndAck {
            update,
            respond_to,
        }) = updates.recv().await
        else {
            panic!("startup admission must project its first durable notice");
        };
        let crate::session::storage::SessionUpdate::Grow(notification) = update else {
            panic!("source inquiry startup must project a Grow session update");
        };
        assert!(matches!(
            notification.update,
            crate::extensions::notification::SessionUpdate::UiNotice(_)
        ));
        respond_to.send(Ok(())).unwrap();
        task.await.unwrap()
    }

    fn inquiry(
        id: usize,
    ) -> (
        crate::coordination::InboundInquiry,
        tokio::sync::oneshot::Receiver<crate::coordination::InquiryOutcome>,
    ) {
        let (progress, _) = tokio::sync::watch::channel(crate::coordination::InquiryPhase::Queued);
        let (respond_to, response) = tokio::sync::oneshot::channel();
        (
            crate::coordination::InboundInquiry {
                authority: crate::coordination::InquiryAuthority::Peer,
                direction: crate::coordination::InquiryDirection::Peer,
                inquiry_id: uuid::Uuid::now_v7().to_string(),
                source_peer_id: "peer".to_owned(),
                source_session_id: format!("source-{id}"),
                source_cwd: "/repo".to_owned(),
                delegated_subagent_task_name: None,
                target_session_id: "test-session".to_owned(),
                question: format!("question-{id}"),
                cancellation: crate::coordination::InquiryCancellation::new(),
                progress,
                respond_to,
            },
            response,
        )
    }

    async fn mock_coordination_rejection(
        actor: &SessionActor,
    ) -> test_support::MockInferenceServer {
        use test_support::{MockInferenceServer, ScriptedResponse};

        let server = MockInferenceServer::start().await.unwrap();
        server.enqueue_response(
            "/v1/chat/completions",
            ScriptedResponse::json(
                400,
                serde_json::json!({
                    "error": {
                        "message": "coordination fixture rejection",
                        "type": "invalid_request_error"
                    }
                }),
            ),
        );
        let mut config = actor.chat_state_handle.get_sampling_config().await.unwrap();
        config.base_url = server.url();
        config.api_backend = sampling_types::ApiBackend::ChatCompletions;
        actor.chat_state_handle.replace_sampling_route(config);
        server
    }

    #[tokio::test(flavor = "current_thread")]
    async fn source_startup_admission_survives_rebuild_without_duplicate_append_or_notice() {
        tokio::task::LocalSet::new()
            .run_until(async {
                use crate::session::storage::{JsonlStorageAdapter, StorageAdapter as _};

                let root = tempfile::tempdir().unwrap();
                let storage = JsonlStorageAdapter::with_root(root.path().to_owned());
                let (mut actor, _gateway) =
                    crate::session::actor::tests::support::build_actor().await;
                let info = actor.session_info.clone();
                storage
                    .init_session(&info, crate::session::persistence::default_model_id())
                    .await
                    .unwrap();

                let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
                let (chat_event_tx, _chat_event_rx) = tokio::sync::mpsc::unbounded_channel();
                Arc::get_mut(&mut actor).unwrap().chat_state_handle =
                    chat_state::ChatStateActor::spawn_from_timeline(
                        Vec::new(),
                        config,
                        Box::new(JsonlTimelinePersistence {
                            storage: storage.clone(),
                            info: info.clone(),
                        }),
                        chat_event_tx,
                        tokio_util::sync::CancellationToken::new(),
                    )
                    .await
                    .unwrap();
                let (updates_tx, mut updates_rx) = tokio::sync::mpsc::unbounded_channel();
                Arc::get_mut(&mut actor)
                    .unwrap()
                    .notifications
                    .persistence_tx = updates_tx;

                let event = crate::coordination::InquiryEvent::OutgoingStarted {
                    inquiry_id: uuid::Uuid::now_v7().to_string(),
                    target_session_id: "target-session".into(),
                    question: "exact question".into(),
                };
                admit_and_ack_startup(&actor, event.clone(), &mut updates_rx)
                    .await
                    .unwrap();
                assert!(updates_rx.try_recv().is_err());

                let loaded = crate::session::storage::load_timeline_by_id_at(
                    info.id.0.as_ref(),
                    root.path(),
                )
                .unwrap()
                .unwrap();
                assert_eq!(
                    loaded
                        .events()
                        .iter()
                        .filter(|event| matches!(
                            crate::coordination::InquiryEvent::from_timeline(event),
                            Ok(Some(
                                crate::coordination::InquiryEvent::OutgoingStarted { .. }
                            ))
                        ))
                        .count(),
                    1,
                    "the accepted startup must have one canonical durable fact"
                );

                let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
                let (chat_event_tx, _chat_event_rx) = tokio::sync::mpsc::unbounded_channel();
                Arc::get_mut(&mut actor).unwrap().chat_state_handle =
                    chat_state::ChatStateActor::spawn_from_timeline(
                        loaded.events().to_vec(),
                        config,
                        Box::new(JsonlTimelinePersistence {
                            storage: storage.clone(),
                            info: info.clone(),
                        }),
                        chat_event_tx,
                        tokio_util::sync::CancellationToken::new(),
                    )
                    .await
                    .unwrap();

                assert_eq!(
                    actor.persist_coordination_inquiry(event.clone()).await,
                    Ok(())
                );
                assert!(
                    updates_rx.try_recv().is_err(),
                    "retry must not project a second notice"
                );
                for conflict in [
                    crate::coordination::InquiryEvent::OutgoingStarted {
                        inquiry_id: match &event {
                            crate::coordination::InquiryEvent::OutgoingStarted {
                                inquiry_id,
                                ..
                            } => inquiry_id.clone(),
                            _ => unreachable!(),
                        },
                        target_session_id: "different-target".into(),
                        question: "exact question".into(),
                    },
                    crate::coordination::InquiryEvent::OutgoingStarted {
                        inquiry_id: match &event {
                            crate::coordination::InquiryEvent::OutgoingStarted {
                                inquiry_id,
                                ..
                            } => inquiry_id.clone(),
                            _ => unreachable!(),
                        },
                        target_session_id: "target-session".into(),
                        question: "different question".into(),
                    },
                ] {
                    let error = actor
                        .persist_coordination_inquiry(conflict)
                        .await
                        .unwrap_err();
                    assert_eq!(
                        error.code,
                        crate::coordination::CoordinationErrorCode::Conflict
                    );
                    assert!(updates_rx.try_recv().is_err());
                }

                let after = crate::session::storage::load_timeline_by_id_at(
                    info.id.0.as_ref(),
                    root.path(),
                )
                .unwrap()
                .unwrap();
                assert_eq!(after.events().len(), loaded.events().len());
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inquiry_restore_closes_only_open_timeline_receipts_without_ui_replay() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let (open, _) = inquiry(1);
                let (completed, _) = inquiry(2);
                actor
                    .record_incoming_coordination_notice(&open)
                    .await
                    .unwrap();
                actor
                    .record_coordination_approval_notice(&open, "approved")
                    .await
                    .unwrap();
                actor
                    .record_incoming_coordination_notice(&completed)
                    .await
                    .unwrap();
                actor
                    .record_coordination_terminal_notice(
                        &completed,
                        &crate::coordination::InquiryOutcome::answered(
                            &completed.inquiry_id,
                            "answer".into(),
                        ),
                        None,
                    )
                    .await
                    .unwrap();
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
                let (mut restored, _gateway) =
                    crate::session::actor::tests::support::build_actor().await;
                let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
                Arc::get_mut(&mut restored).unwrap().chat_state_handle =
                    chat_state::ChatStateActor::spawn_from_timeline(
                        events,
                        config,
                        Box::new(chat_state::NullTimelinePersistence),
                        event_tx,
                        Default::default(),
                    )
                    .await
                    .unwrap();
                let replay = restored
                    .session_dir
                    .join(crate::session::storage::UPDATES_FILE);
                std::fs::write(&replay, "obsolete UI projection").unwrap();
                std::fs::remove_file(&replay).unwrap();
                let pending = restored.pending_coordination_notices().await.unwrap();
                assert_eq!(pending.len(), 1);
                assert_eq!(pending[0].0, open.inquiry_id);
                restored
                    .recover_interrupted_coordination_inquiries()
                    .await
                    .unwrap();
                assert!(
                    restored
                        .pending_coordination_notices()
                        .await
                        .unwrap()
                        .is_empty()
                );
                let after = restored.chat_state_handle.timeline_events().await.unwrap();
                let Some(crate::coordination::InquiryEvent::Incoming { audit, .. }) =
                    crate::coordination::InquiryEvent::from_timeline(after.last().unwrap())
                        .unwrap()
                else {
                    panic!()
                };
                assert_eq!(
                    audit.outcome.unwrap().error.unwrap().code,
                    crate::coordination::CoordinationErrorCode::TargetRestarted
                );
                restored
                    .recover_interrupted_coordination_inquiries()
                    .await
                    .unwrap();
                assert_eq!(
                    restored
                        .chat_state_handle
                        .timeline_events()
                        .await
                        .unwrap()
                        .len(),
                    after.len()
                );
                assert!(
                    !after.iter().any(|event| matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Sideband(_)
                    )),
                    "restore must never replay the inquiry model request"
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn target_queue_is_fifo_and_rejects_the_thirty_third_waiter() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                actor.coordination_inquiry_active.set(true);
                let mut responses = Vec::new();
                for id in 0..crate::coordination::MAX_QUEUED_INQUIRIES {
                    let (inquiry, response) = inquiry(id);
                    actor.enqueue_coordination_inquiry(inquiry).await;
                    responses.push(response);
                }
                let queued_sources: Vec<_> = actor
                    .coordination_inquiries
                    .borrow()
                    .iter()
                    .map(|queued| queued.inquiry.source_session_id.clone())
                    .collect();
                assert_eq!(
                    queued_sources,
                    (0..crate::coordination::MAX_QUEUED_INQUIRIES)
                        .map(|id| format!("source-{id}"))
                        .collect::<Vec<_>>()
                );

                let (overflow, overflow_response) = inquiry(33);
                actor.enqueue_coordination_inquiry(overflow).await;
                let outcome = overflow_response.await.unwrap();
                assert_eq!(
                    outcome.status,
                    crate::coordination::InquiryStatus::Unavailable
                );
                assert_eq!(
                    actor.coordination_inquiries.borrow().len(),
                    crate::coordination::MAX_QUEUED_INQUIRIES
                );
                drop(responses);
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pre_cancelled_inquiry_never_enters_target_queue() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let (inquiry, response) = inquiry(1);
                inquiry
                    .cancellation
                    .cancel(crate::coordination::InquiryCancellationReason::Explicit);

                actor.enqueue_coordination_inquiry(inquiry).await;

                assert!(actor.coordination_inquiries.borrow().is_empty());
                assert!(!actor.coordination_inquiry_active.get());
                assert_eq!(
                    response.await.unwrap().status,
                    crate::coordination::InquiryStatus::Cancelled
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inquiry_preserves_completed_tool_evidence_on_all_backends() {
        use crate::coordination::{InquiryAuthority, InquiryDirection, InquiryStatus};
        use crate::session::actor::tests::support::{create_test_actor, replace_test_surface};
        use sampling_types::{ApiBackend, AssistantItem, ContentPart, ToolCall};
        use test_support::MockInferenceServer;

        const IMAGE: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aO1cAAAAASUVORK5CYII=";

        tokio::task::LocalSet::new().run_until(async {
            for backend in [ApiBackend::ChatCompletions, ApiBackend::Responses, ApiBackend::Messages] {
                for direction in [InquiryDirection::ParentToChild, InquiryDirection::ChildToParent, InquiryDirection::Peer] {
                    // Complete, partially completed, and entirely pending tail batches.
                    for completed in [2, 1, 0] {
                        let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
                        let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                        tokio::task::spawn_local(async move {
                            while let Some(message) = persistence_rx.recv().await {
                                if let PersistenceMsg::SidebandDurablyAndAck { respond_to, .. } = message {
                                    let _ = respond_to.send(Ok(()));
                                }
                            }
                        });
                        let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
                        let server = MockInferenceServer::start().await.unwrap();
                        server.set_response("Answer from the recorded inquiry.");
                        let mut config = actor.chat_state_handle.get_sampling_config().await.unwrap();
                        config.base_url = server.url();
                        config.api_backend = backend.clone();
                        actor.chat_state_handle.replace_sampling_route(config);
                        let call = |id: &str| ToolCall {
                            id: id.into(),
                            name: "read_file".into(),
                            arguments: r#"{"path":"src/reader.rs"}"#.into(),
                        };
                        let mut history = vec![
                            ConversationItem::user("Implement the delegated reader."),
                            ConversationItem::assistant("I will confirm the interface."),
                            ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item("private inquiry reasoning")),
                            ConversationItem::assistant_tool_calls(vec![ToolCall {
                                id: "parent_question".into(),
                                name: "ask_parent".into(),
                                arguments: r#"{"question":"Should the reader expose CpEntryFacts?"}"#.into(),
                            }]),
                            ConversationItem::tool_result("parent_question", r#"{"status":"answered","answer":"Use CpEntryFacts by value."}"#),
                            ConversationItem::Assistant(AssistantItem {
                                content: "The parent answered; inspecting the implementation.".into(),
                                tool_calls: vec![call("read_a"), call("read_b")],
                                model_id: None,
                                model_fingerprint: None,
                                reasoning_effort: None,
                                response_messages: Vec::new(),
                            }),
                        ];
                        if completed >= 1 {
                            history.push(ConversationItem::tool_result_with_images(
                                "read_a", "Reader implementation inspected.",
                                vec![ContentPart::Text { text: "Reader attachment retained.".into() },
                                    ContentPart::Image { url: format!("data:image/png;base64,{IMAGE}").into(), description: None }],
                            ));
                        }
                        if completed == 2 {
                            history.push(ConversationItem::tool_result("read_b", "Reader caller inspected."));
                        }
                        replace_test_surface(&actor.chat_state_handle, history).await;
                        let before = serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap();
                        let (mut question, _) = inquiry(1);
                        question.authority = if direction == InquiryDirection::Peer { InquiryAuthority::Peer } else { InquiryAuthority::Delegation };
                        question.direction = direction;
                        question.source_cwd = actor.session_info.cwd.clone();
                        question.target_session_id = actor.session_info.id.to_string();
                        question.question = "What did you ask the parent, and was it answered?".into();
                        let outcome = tokio::time::timeout(Duration::from_secs(5), actor.handle_coordination_inquiry(&question, &mut None)).await.unwrap();
                        assert_eq!(outcome.status, InquiryStatus::Answered, "{outcome:?}");
                        assert_eq!(serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap(), before);
                        let requests = server.requests();
                        assert_eq!(requests.len(), 1, "inquiries allow one provider attempt");
                        let body = requests[0].body.as_ref().unwrap();
                        let wire = body.to_string();
                        assert!(wire.contains("Should the reader expose CpEntryFacts?"), "{backend:?}/{direction:?}/{completed}: lost completed ask_parent arguments");
                        assert!(wire.contains("Use CpEntryFacts by value."));
                        assert!(wire.contains("The parent answered; inspecting the implementation."));
                        assert!(wire.contains(&question.question));
                        assert!(!wire.contains("private inquiry reasoning"));
                        assert!(body.get("tools").is_none_or(|tools| tools.as_array().is_some_and(Vec::is_empty)));
                        assert!(body.get("tool_choice").is_none());
                        let mut expected = vec!["parent_question"];
                        if completed >= 1 {
                            expected.push("read_a");
                            assert!(wire.contains("src/reader.rs"));
                            assert!(wire.contains("Reader implementation inspected."));
                            assert!(wire.contains("Reader attachment retained."));
                            assert!(wire.contains(IMAGE));
                        }
                        if completed == 2 {
                            expected.push("read_b");
                            assert!(wire.contains("Reader caller inspected."));
                        }
                        assert_inquiry_wire_pairs(body, &expected);
                    }
                }
            }
        }).await;
    }

    fn assert_inquiry_wire_pairs(body: &serde_json::Value, expected: &[&str]) {
        fn collect(value: &serde_json::Value, calls: &mut Vec<String>, results: &mut Vec<String>) {
            match value {
                serde_json::Value::Object(object) => {
                    let call = match value["type"].as_str() {
                        Some("tool_use") => value["id"].as_str(),
                        Some("function_call") => value["call_id"].as_str(),
                        _ => None,
                    };
                    calls.extend(call.map(str::to_owned));
                    if let Some(tools) = value["tool_calls"].as_array() {
                        calls.extend(
                            tools
                                .iter()
                                .map(|call| call["id"].as_str().unwrap().to_owned()),
                        );
                    }
                    let result = match value["type"].as_str() {
                        Some("tool_result") => value["tool_use_id"].as_str(),
                        Some("function_call_output") => value["call_id"].as_str(),
                        _ if value["role"] == "tool" => value["tool_call_id"].as_str(),
                        _ => None,
                    };
                    results.extend(result.map(str::to_owned));
                    for value in object.values() {
                        collect(value, calls, results);
                    }
                }
                serde_json::Value::Array(values) => {
                    for value in values {
                        collect(value, calls, results);
                    }
                }
                _ => {}
            }
        }
        let (mut calls, mut results) = (Vec::new(), Vec::new());
        collect(body, &mut calls, &mut results);
        calls.sort();
        results.sort();
        let mut expected = expected.to_vec();
        expected.sort();
        assert_eq!(calls, expected, "provider tool calls");
        assert_eq!(results, expected, "provider tool results");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inquiry_uses_one_info_request_sideband_without_mutating_parent_surface() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
                let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                let actor = Arc::new(
                    crate::session::actor::tests::support::create_test_actor(
                        0,
                        256_000,
                        85,
                        gateway_tx,
                        persistence_tx,
                    )
                    .await,
                );
                let server = mock_coordination_rejection(&actor).await;
                let before = actor
                    .chat_state_handle
                    .materialize_timeline(actor.session_info.id.to_string())
                    .await
                    .unwrap()
                    .surface;
                let before = serde_json::to_value(before).unwrap();
                let (mut inquiry, _response) = inquiry(1);
                inquiry.source_cwd =
                    crate::coordination::canonical_cwd(Path::new(&actor.session_info.cwd));
                inquiry.target_session_id = actor.session_info.id.to_string();

                let outcome = tokio::time::timeout(
                    Duration::from_secs(5),
                    actor.handle_coordination_inquiry(&inquiry, &mut None),
                )
                .await
                .expect("mock provider rejection must fail promptly");
                assert_eq!(outcome.status, crate::coordination::InquiryStatus::Failed);
                assert_eq!(server.request_count(), 1);
                let after = actor
                    .chat_state_handle
                    .materialize_timeline(actor.session_info.id.to_string())
                    .await
                    .unwrap()
                    .surface;
                let after = serde_json::to_value(after).unwrap();
                assert_eq!(after, before, "sideband must not mutate the parent Surface");

                let mut purposes = Vec::new();
                let mut attempts = 0;
                while let Ok(message) = persistence_rx.try_recv() {
                    if let PersistenceMsg::SidebandDurablyAndAck { event, .. } = message {
                        match event.kind {
                            chat_state::SidebandEventKind::Request(request) => {
                                purposes.push(request.purpose);
                                assert_eq!(request.budget_policy.max_attempts, 2);
                            }
                            chat_state::SidebandEventKind::Attempt(_) => attempts += 1,
                            _ => {}
                        }
                    }
                }
                assert_eq!(purposes, vec![chat_state::SidebandPurpose::InfoRequest]);
                assert_eq!(attempts, 1, "inquiry executes exactly one provider attempt");
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn delegated_inquiry_during_foreground_bypasses_peer_approval_without_injecting_input() {
        tokio::task::LocalSet::new().run_until(async {
            use axum::{Json, Router, response::Sse, response::sse::Event, routing::post};
            let (request_tx, mut request_rx) = tokio::sync::mpsc::unbounded_channel();
            let release = Arc::new(tokio::sync::Notify::new());
            let response_release = Arc::clone(&release);
            let app = Router::new().route("/v1/chat/completions", post(move |Json(body): Json<serde_json::Value>| {
                let request_tx = request_tx.clone();
                let release = Arc::clone(&response_release);
                async move {
                    request_tx.send(body).unwrap();
                    release.notified().await;
                    let chunks = [
                        serde_json::json!({"id":"question", "object":"chat.completion.chunk", "created":0, "model":"test",
                            "choices":[{"index":0,"delta":{"role":"assistant","content":"Use the existing interface."},"finish_reason":"stop"}],
                            "usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}}).to_string(),
                        "[DONE]".into(),
                    ];
                    Sse::new(futures::stream::iter(chunks.into_iter().map(|chunk| Ok::<_, std::convert::Infallible>(Event::default().data(chunk)))))
                }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let (mut actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
            let recorded_sidebands = Arc::new(std::sync::Mutex::new(crate::session::storage::SidebandLedgers::new()));
            let records = Arc::clone(&recorded_sidebands);
            let old_persistence = actor.notifications.persistence_tx.clone();
            let (persistence_tx, mut persistence_rx) = mpsc::unbounded_channel();
            Arc::get_mut(&mut actor).unwrap().notifications.persistence_tx = persistence_tx;
            tokio::task::spawn_local(async move {
                while let Some(message) = persistence_rx.recv().await {
                    if let crate::session::persistence::PersistenceMsg::SidebandDurablyAndAck { event, .. } = &message {
                        records.lock().unwrap().entry(event.sideband_id.clone()).or_default().push(event.clone());
                    }
                    if old_persistence.send(message).is_err() { break; }
                }
            });
            let mut config = actor.chat_state_handle.get_sampling_config().await.unwrap();
            config.base_url = base_url.clone();
            config.api_backend = sampling_types::ApiBackend::ChatCompletions;
            actor.chat_state_handle.update_sampling_config(config);
            let mut route = actor.model_route.snapshot();
            route.sampling_config.base_url = base_url;
            route.sampling_config.api_backend = sampling_types::ApiBackend::ChatCompletions;
            actor.model_route.replace(route.model_id, route.sampling_config);
            crate::session::actor::tests::support::begin_test_active_causal_turn(&actor).await;
            actor.chat_state_handle.record_timeline_event_durably(
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    Box::new(chat_state::SubagentSpawnEvent {
                        subagent_id: "active-child".into(),
                        child_session_id: "child-session".into(),
                        origin_tool_call_id: None,
                        security_parent_session_id: actor.session_info.id.to_string(),
            parent_step_id: None,
                        subagent_type: "explore".into(),
                        description: "keep running in the background".into(),
                        prompt: "inspect the target".into(),
                        execution_recipe: serde_json::json!({}),
                        context_source: chat_state::SubagentContextSource::New,
                        source_ref: None,
                        context_normalized: false,
                        resumed_from: None,
                        parent_prompt_id: None,
                        capability_mode: None,
                        birth_mcp_servers: Some(Default::default()),
                        permission_mode: None,
                        effective_permission_mode: None,
                        workflow_run_id: None,
                        goal_id: None,
                        goal_definition_revision: None,
                        surface_completion: true,
                        child_cwd: actor.session_info.cwd.clone(),
                        worktree_path: None,
                        effective_model_id: "test-model".into(),
                        model_transport_key: sampling_types::ModelImageInputKey::new(
                            "test-model",
                            "chat_completions",
                            "test-endpoint",
                        ),
                        reasoning_effort: None,
                    }),
                )),
            ).await.unwrap();
            actor.notifications.gateway_enabled.store(false, std::sync::atomic::Ordering::Release);
            let before = serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap();
            let (mut question, response) = inquiry(7);
            question.authority = crate::coordination::InquiryAuthority::Delegation;
            question.source_cwd = "/delegated-worktree".into();
            question.target_session_id = actor.session_info.id.to_string();
            let id = question.inquiry_id.clone();
            actor.enqueue_coordination_inquiry(question).await;
            let request = tokio::time::timeout(Duration::from_secs(5), request_rx.recv()).await.unwrap().unwrap();
            assert!(request.get("tools").is_none_or(|tools| tools.as_array().is_some_and(Vec::is_empty)));
            assert!(actor.state.lock().await.foreground.regular().is_some());
            assert!(actor.coordination_inquiry_active.get());
            release.notify_one();
            let outcome = tokio::time::timeout(Duration::from_secs(5), response).await.unwrap().unwrap();
            assert_eq!(outcome.status, crate::coordination::InquiryStatus::Answered);
            assert_eq!(outcome.answer.as_deref(), Some("Use the existing interface."));
            assert!(actor.state.lock().await.foreground.regular().is_some());
            assert_eq!(serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap(), before);
            assert!(actor.pending_interjections.is_empty());
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let receipts: Vec<_> = events.iter().filter_map(|event| match crate::coordination::InquiryEvent::from_timeline(event).unwrap() {
                Some(crate::coordination::InquiryEvent::Incoming { inquiry_id, audit }) => Some((inquiry_id, audit)),
                _ => None,
            }).collect();
            assert!(receipts.len() >= 3);
            assert!(receipts.iter().all(|(key, _)| key == &id), "all UI lifecycle updates must use one inquiry row identity");
            assert!(receipts.last().unwrap().1.outcome.is_some());
            assert!(events.iter().any(|event| matches!(&event.kind, chat_state::TimelineEventKind::Sideband(spawn) if spawn.purpose == chat_state::SidebandPurpose::InfoRequest)));
            assert!(events.iter().any(|event| matches!(&event.kind,
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(spawn))
                    if spawn.subagent_id == "active-child")));
            assert!(!events.iter().any(|event| matches!(&event.kind,
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(ended))
                    if ended.subagent_id == "active-child")));
            let timeline = chat_state::Timeline::from_events(events.clone()).unwrap();
            let ledgers = recorded_sidebands.lock().unwrap().clone();
            crate::session::storage::validate_sideband_ledgers(&actor.session_info.id.to_string(), &timeline, &ledgers).unwrap();
            // The same public answer cannot be attached to a different question/source.
            let mut altered = events.clone();
            for event in &mut altered {
                if let Some(crate::coordination::InquiryEvent::Incoming { inquiry_id, mut audit }) = crate::coordination::InquiryEvent::from_timeline(event).unwrap()
                    && audit.answer_context.is_some()
                {
                    audit.question.push_str(" different question");
                    event.kind = crate::coordination::InquiryEvent::Incoming { inquiry_id, audit }.timeline_kind();
                }
            }
            let changed = chat_state::Timeline::from_events(altered).unwrap();
            assert!(crate::session::storage::validate_sideband_ledgers(&actor.session_info.id.to_string(), &changed, &ledgers).is_err());
            for corruption in ["missing-proof", "terminal-version", "payload", "asker", "owner"] {
                let mut corrupt = events.clone();
                for event in &mut corrupt {
                    if corruption == "missing-proof" {
                        if let Some(crate::coordination::InquiryEvent::Incoming { inquiry_id, mut audit }) = crate::coordination::InquiryEvent::from_timeline(event).unwrap()
                            && audit.answer_context.is_some()
                        {
                            audit.answer_context = None;
                            event.kind = crate::coordination::InquiryEvent::Incoming { inquiry_id, audit }.timeline_kind();
                        }
                    }
                    if let chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Received { id, owner_session_id, source, source_version, payload_ref }) = &mut event.kind
                        && matches!(source, chat_state::NotificationSource::InquiryAnswered { .. })
                    {
                        match corruption {
                            "terminal-version" => *source_version = chat_state::NotificationSourceVersion::Opaque { value: "wrong-terminal".into() },
                            "payload" => { payload_ref.blake3 = blake3::hash(b"altered").to_hex().to_string(); payload_ref.bytes = 7; },
                            "asker" => if let chat_state::NotificationSource::InquiryAnswered { source_session_id, .. } = source { *source_session_id = "wrong-asker".into(); },
                            "owner" => *owner_session_id = "wrong-owner".into(),
                            _ => {},
                        }
                        *id = chat_state::notification_id(owner_session_id, source, source_version).unwrap();
                    }
                }
                let corrupt = chat_state::Timeline::from_events(corrupt).unwrap();
                assert!(crate::session::storage::validate_sideband_ledgers(&actor.session_info.id.to_string(), &corrupt, &ledgers).is_err(), "{corruption}");
            }
            let mut duplicated = timeline.clone();
            duplicated.record(crate::coordination::InquiryEvent::Incoming { inquiry_id: id.clone(), audit: receipts.last().unwrap().1.clone() }.timeline_kind()).unwrap();
            assert!(crate::session::storage::validate_sideband_ledgers(&actor.session_info.id.to_string(), &duplicated, &ledgers).is_err());
            // Rebuild the canonical prefix after terminal commit but before Received.
            let interrupted = events.iter().take_while(|event| !matches!(event.kind,
                chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Received { source: chat_state::NotificationSource::InquiryAnswered { .. }, .. })))
                .cloned().collect::<Vec<_>>();
            let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
            let (mut restored, _) = crate::session::actor::tests::support::build_actor().await;
            let (event_tx, _) = mpsc::unbounded_channel();
            let restored_mut = Arc::get_mut(&mut restored).unwrap();
            restored_mut.session_info = actor.session_info.clone();
            restored_mut.chat_state_handle = chat_state::ChatStateActor::spawn_from_timeline(interrupted.clone(), config.clone(), Box::new(chat_state::NullTimelinePersistence), event_tx, Default::default()).await.unwrap();
            restored.state.lock().await.runtime_suspend.store(true, std::sync::atomic::Ordering::Release);
            restored.reconcile_coordination_answers().await.unwrap();
            assert!(restored.chat_state_handle.pending_notifications().await.unwrap().is_empty());
            assert_eq!(restored.coordination_answer_next_seq.get(), 0, "closed execution gate must retain the canonical frontier");
            restored.state.lock().await.runtime_suspend.store(false, std::sync::atomic::Ordering::Release);
            restored.reconcile_coordination_answers().await.unwrap();
            restored.reconcile_coordination_answers().await.unwrap();
            assert_eq!(restored.chat_state_handle.pending_notifications().await.unwrap().len(), 1);
            assert!(request_rx.try_recv().is_err(), "recovery must not replay Sideband sampling");
            let (mut fork, _) = crate::session::actor::tests::support::build_actor().await;
            let (event_tx, _) = mpsc::unbounded_channel();
            Arc::get_mut(&mut fork).unwrap().session_info.id = acp::SessionId::new("fork-actor");
            Arc::get_mut(&mut fork).unwrap().chat_state_handle = chat_state::ChatStateActor::spawn_from_timeline(interrupted, config, Box::new(chat_state::NullTimelinePersistence), event_tx, Default::default()).await.unwrap();
            fork.reconcile_coordination_answers().await.unwrap();
            assert!(fork.chat_state_handle.pending_notifications().await.unwrap().is_empty(), "fork does not acquire original target's obligation");
            assert!(receipts.last().unwrap().1.answer_context.is_some());
            assert!(!*actor.parent_message_interrupt.borrow());
            let pending = actor.chat_state_handle.pending_notifications().await.unwrap();
            assert_eq!(pending.iter().filter(|receipt| matches!(receipt.source, chat_state::NotificationSource::InquiryAnswered { .. })).count(), 1);
            assert!(actor.drain_active_notifications().await.unwrap());
            assert!(!actor.drain_active_notifications().await.unwrap());
            let request = actor.chat_state_handle.build_request("parent", vec![], None, None, None).await.unwrap();
            let surface = serde_json::to_value(&request.items).unwrap().to_string();
            assert!(surface.contains("question-7") && surface.contains("Use the existing interface."));
            assert!(surface.contains("Sideband") && surface.contains("not a foreground decision or human permission"));
            assert!(request.items.iter().any(|item| matches!(item,
                sampling_types::ConversationItem::AgentMessage(batch)
                    if batch.messages.iter().any(|message| message.receipt_id == pending[0].id && message.reply_to.is_none())
            )));
            // The real provider adapter receives that same attributed context.
            let client = actor.prepare_chat_completion(false).await.unwrap();
            let response_future = client.conversation_collect(request);
            tokio::pin!(response_future);
            let next_request = tokio::select! {
                request = request_rx.recv() => request.unwrap(),
                result = &mut response_future => panic!("provider finished before request inspection: {result:?}"),
            };
            let wire = next_request.to_string();
            assert!(wire.contains("question-7") && wire.contains("Use the existing interface."));
            release.notify_one();
            response_future.await.unwrap();
            // A second real inquiry returns the same prose, but its proof cannot
            // be replaced by the first inquiry's result reference.
            let (mut other, other_response) = inquiry(8);
            other.authority = crate::coordination::InquiryAuthority::Delegation;
            other.target_session_id = actor.session_info.id.to_string();
            actor.enqueue_coordination_inquiry(other).await;
            tokio::time::timeout(Duration::from_secs(5), request_rx.recv()).await.unwrap().unwrap();
            release.notify_one();
            assert_eq!(other_response.await.unwrap().answer.as_deref(), Some("Use the existing interface."));
            let all_events = actor.chat_state_handle.timeline_events().await.unwrap();
            let later_proof = all_events.iter().find_map(|event| {
                let Some(crate::coordination::InquiryEvent::Incoming { audit, .. }) = crate::coordination::InquiryEvent::from_timeline(event).unwrap() else { return None; };
                (audit.question == "question-8").then_some(audit.answer_context).flatten()
            }).unwrap();
            let mut forward = all_events.clone();
            for event in &mut forward {
                if let Some(crate::coordination::InquiryEvent::Incoming { inquiry_id, mut audit }) = crate::coordination::InquiryEvent::from_timeline(event).unwrap()
                    && audit.question == "question-7" && audit.answer_context.is_some()
                {
                    audit.question = "question-8".into(); audit.source_session_id = "source-8".into(); audit.source_cwd = "/repo".into();
                    audit.answer_context = Some(later_proof.clone());
                    event.kind = crate::coordination::InquiryEvent::Incoming { inquiry_id, audit }.timeline_kind();
                }
            }
            let forward = chat_state::Timeline::from_events(forward).unwrap();
            let error = crate::session::storage::validate_sideband_ledgers(&actor.session_info.id.to_string(), &forward, &recorded_sidebands.lock().unwrap()).unwrap_err();
            assert!(error.to_string().contains("InfoRequest Sideband"), "{error}");
            let mut swapped = all_events;
            let first_ref = receipts.last().unwrap().1.answer_context.as_ref().unwrap().result_ref.clone();
            for event in &mut swapped {
                if let Some(crate::coordination::InquiryEvent::Incoming { inquiry_id, mut audit }) = crate::coordination::InquiryEvent::from_timeline(event).unwrap()
                    && audit.question == "question-8" && audit.answer_context.is_some()
                {
                    audit.answer_context.as_mut().unwrap().result_ref = first_ref.clone();
                    event.kind = crate::coordination::InquiryEvent::Incoming { inquiry_id, audit }.timeline_kind();
                }
            }
            let wrong_question = chat_state::Timeline::from_events(swapped).unwrap();
            let error = crate::session::storage::validate_sideband_ledgers(&actor.session_info.id.to_string(), &wrong_question, &recorded_sidebands.lock().unwrap()).unwrap_err();
            assert!(error.to_string().contains("different question or source"), "{error}");
            server.abort();
        }).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cross_workspace_inquiry_fails_closed_without_online_ui() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                actor
                    .notifications
                    .gateway_enabled
                    .store(false, std::sync::atomic::Ordering::Release);
                let (mut inquiry, _response) = inquiry(1);
                inquiry.source_cwd = "/different-workspace".to_owned();
                inquiry.target_session_id = actor.session_info.id.to_string();

                let outcome = actor.handle_coordination_inquiry(&inquiry, &mut None).await;

                assert_eq!(outcome.status, crate::coordination::InquiryStatus::Rejected);
                assert!(outcome.error.unwrap().message.contains("no online UI"));
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cross_workspace_approval_offers_only_one_shot_choices() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (gateway_tx, mut gateway_rx) = tokio::sync::mpsc::unbounded_channel();
                let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                let actor = Arc::new(
                    crate::session::actor::tests::support::create_test_actor(
                        0,
                        256_000,
                        85,
                        gateway_tx,
                        persistence_tx,
                    )
                    .await,
                );
                let server = mock_coordination_rejection(&actor).await;
                let observed = Arc::new(std::sync::Mutex::new(None));
                let observed_for_gateway = Arc::clone(&observed);
                tokio::task::spawn_local(async move {
                    while let Some(message) = gateway_rx.recv().await {
                        if let acp_transport::AcpClientMessage::RequestPermission(args) = message {
                            *observed_for_gateway.lock().unwrap() = Some(args.request.clone());
                            let response = acp::RequestPermissionResponse::new(
                                acp::RequestPermissionOutcome::Selected(
                                    acp::SelectedPermissionOutcome::new(
                                        acp::PermissionOptionId::new("allow-once"),
                                    ),
                                ),
                            );
                            let _ = args.response_tx.send(Ok(response));
                        }
                    }
                });
                let (mut inquiry, _response) = inquiry(1);
                inquiry.source_cwd = "/different-workspace".to_owned();
                inquiry.target_session_id = actor.session_info.id.to_string();

                let outcome = tokio::time::timeout(
                    Duration::from_secs(5),
                    actor.handle_coordination_inquiry(&inquiry, &mut None),
                )
                .await
                .unwrap();

                assert_eq!(outcome.status, crate::coordination::InquiryStatus::Failed);
                assert_eq!(server.request_count(), 1);
                let request = observed.lock().unwrap().clone().unwrap();
                assert_eq!(request.session_id, actor.session_info.id);
                assert_eq!(request.options.len(), 2);
                assert_eq!(request.options[0].option_id.0.as_ref(), "allow-once");
                assert_eq!(request.options[1].option_id.0.as_ref(), "reject-once");
                assert!(request.options.iter().all(|option| {
                    !matches!(option.kind, acp::PermissionOptionKind::AllowAlways)
                }));
                let mut saw_info_request = false;
                while let Ok(message) = persistence_rx.try_recv() {
                    if let PersistenceMsg::SidebandDurablyAndAck { event, .. } = message
                        && let chat_state::SidebandEventKind::Request(request) = event.kind
                    {
                        saw_info_request =
                            request.purpose == chat_state::SidebandPurpose::InfoRequest;
                    }
                }
                assert!(saw_info_request, "approved inquiry must reach its sideband");
            })
            .await;
    }
}
