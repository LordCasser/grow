# Design and evidence

`SessionActor::pending_coordination_notices` folds incoming events by source peer and inquiry id, retains the latest nonterminal audit and omits completed inquiries. `record_coordination_approval_notice` records the same-workspace approval before starting inference. `publish_coordination_state` sends that audit as transient; `IncomingInquiryAudit::notice` chooses `inquiry approval` when approval exists. The failing fixture captured exactly this state, including `approved (same workspace)` and `outcome: null`.

The existing assertion required an earlier phase even after the fixture waited for model admission. Replace it with exact current-state assertions, preserving the same InquiryId, source session, question and one-shot model request. Notification delivery uses a separate asynchronous channel, so use the existing bounded `eventually` helper before inspecting its contents.
