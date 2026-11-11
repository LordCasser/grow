# Tasks

- [x] Trace the durable `InquiryAnswered` receipt, current NotificationDrain User-text projection, and existing runtime AgentMessage canonical item across restore/replay.
- [x] Project successful target-side Q&A using the existing receipt and typed AgentMessage payload with requester/source session, inquiry ID, answering session, frozen Sideband proof, and complete question/answer.
- [x] Use existing paired host `receive_agent_message` call/result for Chat Completions and Messages; preserve the Responses attributed-context fallback without synthetic model calls or reasoning.
- [x] Preserve the asking agent's original tool call and result/receipt without duplication; keep previously consumed User-form inquiry history readable while new emissions are typed.
- [x] Verify failure/cancellation has no successful Q&A projection, restore/replay does not duplicate or rerun, and projection does not wake, steer, start a run, grant authority, or create reply rights.
- [x] Record verification and review results before marking implementation tasks complete; update developer documentation if the implemented behavior requires it.
