# Design
Normal admission stamps its prompt index before FIFO/direct/notification commit. Resolve the unique non-synthetic User item for that index in the existing materialized Surface and use its stable SurfaceId event. Missing/ambiguous/misaligned identity skips scheduling and leaves the route available; no text matching or Timeline tail fallback.

Direct bash-command input has no prompt index. Extend push_user_message_durably to return the TimelineEvent it already commits; acknowledgement failures remain errors and commit ordering is unchanged. This caller passes the returned event sequence directly to the common title scheduler. Callers needing acknowledgement only explicitly discard the event. This avoids a new source store or fabricated prompt index for commands.

The common scheduler claims the one-shot route only once the input identity is known. Existing Sideband lifecycle and canonical user-title priority remain intact. Tests construct actual Timeline user/notification/control events and verify selection independently of the tail; chat-state tests verify the returned event equals persisted evidence and remains stable after later messages.

# Verification scope
No full model-server title generation or FIFO notification scheduling integration is claimed by helper tests. The shared admission source stamp and callers are also inspected. Initial compile was deliberately interrupted after discovering the direct-command caller, before any test result; edits occurred only after termination.
