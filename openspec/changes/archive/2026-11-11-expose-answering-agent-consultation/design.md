# Design

## Canonical source and projection

The durable `InquiryAnswered` receipt remains authoritative for the successful Sideband exchange. Reuse that receipt and its existing `AgentMessage` canonical item to represent the answering agent's consultation context. The typed payload must preserve the requester/source session, inquiry ID, answering session, the frozen Sideband proof, and the complete question and answer. It must be an attributed Q&A fact, not a fabricated foreground assistant tool invocation or model-authored reasoning.

The current projection path presents this successful Q&A as generic NotificationDrain User text. Change the new projection to consume the canonical typed AgentMessage derived from the existing receipt, preserving the same durable receive/consume and recovery boundary. Do not create a parallel Q&A store, a new send operation, or a second receipt.

## Provider adapters

For Chat Completions and Messages, use the existing paired host `receive_agent_message` call/result representation so the answering model sees the attributed Q&A as a tool result. For Responses, retain the existing attributed-context adapter fallback. That adapter must preserve complete Q&A and provenance without fabricating an executable call, model-authored tool call, or reasoning item.

The asking agent's original ask tool call and its result/receipt remain its only context for the exchange. The target-side typed projection must not add a second result to the asker or alter existing live source receipts.

## Boundary behavior

Only a successful durable `InquiryAnswered` fact produces this target-side context. Failures, timeouts, refusals, or cancellation without a successful answer do not produce Q&A context. The answering agent consumes the item at the existing eligible context boundary. It does not initiate a turn, wake or steer an idle or active session, change tool permissions, or create reply authority. A later reply or message remains an independent operation under existing authorization.

Existing consumed User-form inquiry history remains readable during restore and replay. New target-side emissions use the typed AgentMessage representation. Recovery reuses the receipt and consumption state and does not re-run the Sideband or duplicate the context item.

## Compatibility and non-goals

This is a projection change only. It does not change Sideband snapshots, answers, inquiry IDs, target execution, source tool results, external coordination responses, or message APIs. The typed item describes what was asked and what the Sideband answered; it does not assert that the answering agent performed a foreground action or newly verified the answer.
