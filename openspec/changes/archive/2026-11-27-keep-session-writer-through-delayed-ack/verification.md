# Incident and regression evidence

## Observations from the affected v2.6.19 process

- The last tool call in the root journal used `spawn_subagent(resume_from=…)`. The source child journal measured 1,887,106 bytes. The root journal measured 786,579,557 bytes.
- Local time 14:19:16.199, Shell logged a successful background spawn tool return. The last committed domain record, sequence 90,754, contains the matching tool result, prepared at 14:19:16.201. The next expected tool completion is absent. The later presentation records belong to a different ongoing command. The embedded process reported prompt execution failure at 14:19:50.601. At 14:19:58.125, the coordinator reached model override resolution after source recovery. At 14:19:58.154 Pager recorded a cancelled prompt. The same process rejected the root ID again at 17:10.
- Process allocator samples at 14:19:17 and 14:19:47 show allocated memory growing from 4,530,060,800 to 8,796,493,928 bytes. This is a process-wide observation, not an allocation trace. The journal timestamp is event creation time and does not timestamp fsync or acknowledgement delivery.

## Causal mechanism

In v2.6.19 the embedded Shell created by `spawn_agent_thread_direct` uses a single-thread runtime. The delegate coordinator called the synchronous `durable_resume_source_for` on that runtime. It reconstructed the complete parent timeline to validate the delegation. The root persistence worker was scheduled on the same runtime. The session thread and its ChatState writer timer could continue while the coordinator occupied the thread that services acknowledgements.

The previous writer started a 30-second timer for every ordinary append. If an acknowledgement remained outstanding it abandoned the receiver, marked the writer poisoned and closed the ChatState event channel. Shell handles that channel closing by retiring the session and settling outstanding prompt responders. A removed responder maps to the ACP cancelled stop reason. The supervisor removes the retired handle, so the next lookup by an open Pager fails. The journal outlives this sequence. The old log did not record the exact outstanding sequence or acknowledgement error. The site observed and the reproduced failure mechanism support the attribution, while journal order alone cannot identify which acknowledgement missed the deadline.

Source recovery and worker scheduling now follow the [indexed delegate resume change](../2026-11-27-optimize-indexed-subagent-resume/measurements.md). This change independently corrects the writer's response to delays.

## Regression verification

Before the writer fix the slow ordinary acknowledgement regression failed with the actor already closed. Three corresponding expired-before-dispatch tests also failed. After the fix a delayed (45 seconds of Tokio time) acknowledgement leaves the original write and following command serialized, accepts one commit, and allows the next sequence. A request expiring in the mailbox leaves the sequence available. Reported transient I/O errors and lost acknowledgements retain the bounded retry policy. Explicit in-flight barrier deadlines, storage-full errors, invalid data, and cancellation retain their failure semantics.

The Shell-level test runs the root command loop with ChannelTimelinePersistence and a controlled persistence receiver. Withholding the first real foreground turn's terminal acknowledgement for 45 seconds of Tokio time leaves the prompt pending. Returning success completes that turn and a second /context request through the same root command mailbox, without session/load or a model request.

Rust 1.93.1 offline, locked workspace tests: 682 ChatState tests passed, three opt-in tests skipped. All 32 selected Shell regressions passed, including the live root acknowledgement test, real tool failure handling, cancellation, child usage and publication receipts. Formatting checks for both packages and git diff --check passed.

OpenSpec validation passed all 19 active items before archive, all 18 remaining items after archive and all 998 archived changes.
