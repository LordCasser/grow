# C4 minimum implementation and verification boundary

This is a source-based implementation/review plan, not a test result. No production code was changed and no Cargo command was run while preparing it. C4 remains separate from C3 and C2.

## Ownership and API

`ChatStateActor::ensure_conversation_integrity_durably` in `chat-state/src/actor/mutations.rs` has two callers: `build_conversation_request` and `push_user_message_durably`.

The minimum reuse API is `Result<Option<Vec<ConversationItem>>, TimelineWriteError>`:

- On no repair, return `Some(conversation)`, reusing the existing full Surface copy.
- On repair, keep the existing durable replacement and its acknowledgement, then return `None`. Do not clone another Surface inside this helper.
- The request builder resolves `None` to a copy of the now-committed Surface before applying request-only projections. The user-message caller discards the optional reusable copy, retaining its current clone cost even when repair was necessary.

The Option means only that an owned copy is available for reuse. It does not represent whether persistence succeeded: errors still propagate through Result. No cache, dirty bit, new public type or separate repair predicate is needed. The buffered helper remains unchanged.

## Memory and Goal ordering

The builder should obtain its owned `items` immediately after the durable integrity helper. Preserve the existing reminder trim and duplicate check. For a new reminder, prepare/commit the same typed MemoryContext item and append that item to `items` only after commit succeeds. A clone for the event plus ownership for `items` is sufficient. If the reminder already exists, append nothing.

`Timeline::apply_messages` handles this Messages/Append as an exact Surface append; it does not activate additional contexts. The serialized actor command cannot interleave another command during these awaits. After successful repair and memory commits, the owned items therefore match the authoritative Surface.

Keep Goal projection after both durable boundaries, followed by image-description/image-budget projection as today. `project_conversation_for_goal_scope` rewrites only the owned Vec's contents in place and preserves item positions. It must not mutate Timeline facts, retire the existing source IDs or persist Goal shadows. Do not move the canonical Surface out with `mem::take`, nor return a repaired request before its ACK.

## Minimum tests in actor/tests.rs

Insert the request-time durability tests beside `persistent_memory_context_retries_an_uncertain_commit`, after the existing memory-context request tests. Reuse `TestHarness`, `MockPersistenceReceiver::next_timeline_ack`, the cancellation token and existing persistence inspection helpers.

1. **Actual request-time repair, retry, then memory ACK.** Start with `TestHarness::with_manual_timeline_ack_after(vec![], 1)`. Publicly submit a well-formed Assistant with an unanswered tool call through `push_response_durably`; the one automatic live ACK admits the raw response while deliberately leaving its healthy pending call open. Spawn `build_request` with a nonempty memory reminder. The first held event must be IntegrityRepair; the build must remain pending and no MemoryContext event may have been submitted. Fail that ACK once, compare the entire retry event with the original, and confirm the build remains pending. ACK the retry; the next held event must be MemoryContext and the build must still remain pending. ACK memory, then compare complete request items with the current Surface and cold `Timeline::from_events` projection, including one synthetic result and exactly one reminder. Rebuild with the same reminder and verify no new repair/memory event or duplicate reminder.
2. **Cancellation at either boundary.** Reuse the same public setup and parameterize cancellation while holding repair ACK, or after repair ACK while holding memory ACK. Cancel the harness token and require the build to return an error, never a request. Do not query a poisoned/closed actor or treat every emitted persistence attempt as committed. Inspect attempted-event order, and construct any cold replay assertion only from the prefix whose ACKs the fixture actually confirmed. Existing persistence retry semantics remain unchanged.
3. **Healthy reuse and request-only Goal projection.** Extend or accompany `final_request_projection_accounts_for_goal_shadows_and_json_schema`: use old/current Goal directives plus a memory reminder; verify the obsolete directive is shadowed only in the request, the current directive remains verbatim, persisted Surface retains the original obsolete body, and a repeated identical reminder yields the same request items without another append. Existing no-memory and multiple-tool-result request tests cover the remaining healthy shapes.

`build_request_repairs_dangling_tool_calls` and `dangling_tool_calls_repair_is_consistent_between_state_and_request` initialize damaged history through a constructor that already repairs it. They are useful consistency tests but cannot replace test 1's request-time ACK proof. `push_user_message_durably_waits_for_timeline_commit` and existing durable-user retry coverage should remain green for the second caller; no test should demand a newly returned Vec from that caller.

## Measurement

Use immutable synthetic healthy histories at 128/512/2048/8192 items, including ordinary text, tool-call/result batches and image Arc payloads. Compare the old double-copy request preparation with the new single-copy preparation in the same process; keep fixture construction and equality checks outside timing. Record that the change eliminates one full ConversationItem clone pass on healthy builds; Arc payload bytes are not themselves deep-copied. A pure-kernel probe is not a substitute for request/Surface equality or the ACK tests, and its timing must not be presented as end-to-end production throughput.
