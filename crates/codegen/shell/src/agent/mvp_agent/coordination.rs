use super::*;

impl MvpAgent {
    pub(super) async fn ensure_coordination_started(&self) -> bool {
        match self.coordination.ensure_started().await {
            Ok(()) => {
                self.publish_coordination_snapshot().await;
                self.start_coordination_publisher();
                true
            }
            Err(error) => {
                tracing::warn!(error = %error, "local agent coordination is unavailable");
                false
            }
        }
    }

    fn start_coordination_publisher(&self) {
        if self.coordination_publisher_started.replace(true) {
            return;
        }
        let mut inquiry_rx = self
            .coordination
            .take_inquiry_receiver()
            .expect("coordination inquiry receiver is taken once with the publisher");
        let inquiry_agent = LocalRef::new(self);
        tokio::task::spawn_local(async move {
            while let Some(inquiry) = inquiry_rx.recv().await {
                let target_id = acp::SessionId::new(inquiry.target_session_id.clone());
                let target = inquiry_agent
                    .get()
                    .sessions
                    .borrow()
                    .get(&target_id)
                    .cloned();
                if let Some(target) = target {
                    if let Err(error) = target
                        .cmd_tx
                        .send(SessionCommand::RunCoordinationInquiry { inquiry })
                    {
                        let SessionCommand::RunCoordinationInquiry { inquiry } = error.0 else {
                            unreachable!("coordination dispatcher sent one command variant");
                        };
                        reject_unavailable(inquiry);
                    }
                } else {
                    reject_unavailable(inquiry);
                }
            }
        });
        let agent_ref = LocalRef::new(self);
        tokio::task::spawn_local(async move {
            loop {
                tokio::select! {
                    _ = agent_ref.get().coordination.cancelled() => break,
                    _ = tokio::time::sleep(crate::coordination::HEARTBEAT_INTERVAL) => {
                        agent_ref.get().publish_coordination_snapshot().await;
                    }
                }
            }
        });
    }

    pub(super) async fn publish_coordination_snapshot(&self) {
        let sessions: Vec<_> = self
            .sessions
            .borrow()
            .iter()
            .map(|(id, handle)| (id.clone(), handle.clone()))
            .collect();
        let mut snapshots = Vec::with_capacity(sessions.len());
        for (id, handle) in sessions {
            if handle.cmd_tx.is_closed() {
                continue;
            }
            let active_subagents = self.active_subagent_count(id.0.as_ref()).await;
            snapshots.push(crate::coordination::LocalSessionSnapshot {
                session_id: id.0.to_string(),
                canonical_cwd: crate::coordination::canonical_cwd(
                    std::path::Path::new(&handle.info.cwd),
                ),
                main_agent: handle.agent_profile.name(),
                activity: self.resident_activity(&id),
                subagents: crate::coordination::SubagentStats {
                    active: active_subagents,
                },
            });
        }
        self.coordination.publish_sessions(snapshots);
    }

    async fn active_subagent_count(&self, parent_session_id: &str) -> usize {
        use tools::implementations::grow_build::task::types::{
            SubagentEvent, SubagentListActiveRequest,
        };

        let (respond_to, response) = tokio::sync::oneshot::channel();
        if self
            .subagent_event_tx
            .send(SubagentEvent::ListActive(SubagentListActiveRequest {
                parent_session_id: parent_session_id.to_owned(),
                respond_to,
            }))
            .is_err()
        {
            return 0;
        }
        tokio::time::timeout(std::time::Duration::from_millis(500), response)
            .await
            .ok()
            .and_then(Result::ok)
            .map_or(0, |subagents| subagents.len())
    }
}

fn reject_unavailable(inquiry: crate::coordination::InboundInquiry) {
    let _ = inquiry.respond_to.send(
        crate::coordination::InquiryOutcome::terminal(
            inquiry.inquiry_id,
            crate::coordination::InquiryStatus::Unavailable,
            "target session is unavailable",
        ),
    );
}
