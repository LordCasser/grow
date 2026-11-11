# Expose completed Sideband consultations to the answering agent

## Why

When a primary or subagent answers an `ask_*` inquiry through a frozen-context Sideband, the durable `InquiryAnswered` Q&A currently reaches the answering session through a generic NotificationDrain User-text projection. The answering agent should be able to see who asked, what they asked, and the answer it produced in its own subsequent context, with the same provenance already held by the durable inquiry receipt.

## What changes

- Project a successful `InquiryAnswered` receipt as the existing runtime `AgentMessage` canonical item containing the complete attributed question and answer, requester session, inquiry ID, answering session, and frozen Sideband proof.
- Reuse the existing provider context adapters: paired host `receive_agent_message` call/result on Chat Completions and Messages; the established attributed-context fallback on Responses.
- Keep the asking agent's original tool call and result/receipt unchanged. Do not append a duplicate question or answer to the asker.
- Preserve consumed User-form historical inquiry context as readable history; emit new answering-agent context using the typed AgentMessage form.
- Do not fabricate model-authored calls or reasoning, start a new run, wake or steer the session, or grant authorization or reply authority.

## Scope

This changes only how successful Sideband Q&A is projected into the current answering agent's context. Inquiry execution, durable receipt identity and recovery, current ask tool results, and all message/send behavior remain as specified by their existing contracts.

## Impact

Affected capabilities: `local-coordination` and `session-timeline`.
