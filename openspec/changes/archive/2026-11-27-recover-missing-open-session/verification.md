# Verification

The local unified log for the affected v2.6.19 tab shows the same embedded Shell/Pager PID before and after the failed requests. The long turn closed at 14:19 local time. Five prompt attempts at 17:10 each reached Shell and failed handle lookup before admission. The source directory still contained a 786,579,557-byte journal, ending at physical sequence 131719. Earlier in the same process five child launches reported 30-second publication deadlines. The diagnostics available at the time do not establish the initial reason the root actor stopped.

Session Info's durable lookup uses an independent 16 MiB read budget. Shell returns a typed lookup outcome on failure. The old Pager attempted to deserialize its absence-of-data response as a complete session before reading that status.

An isolated APFS copy of the affected session was opened via `resume_session_with_indexed_history` in the incident-version storage loader, with a private storage root. The copy had a new inode, forcing verification of the canonical source. It loaded domain event 90,754 and reconstructed 900 model Surface items in 145.83 seconds. The test ran storage recovery only. It did not launch a model, run project tools or change the live session. This demonstrates why treating a 30-second UI timer as load failure can strand a healthy long session.

The Pager regression group passed 12 tests with one opt-in benchmark skipped. It covers a bound prompt rejection, draft retention, a stale or child model rejection, rollback on a failed reload, precise ACP error classification and the unavailable Session Info outcome. An adjacent control/reconnect group passed 10 tests, overlapping the Pager regression group. The Shell regression selection passed 491 tests with five opt-in tests skipped. The missing-resident path uses the ordinary load completion with no fixed 30-second deadline. The existing leader-gap reload still uses its previous timeout.

OpenSpec strict validation passed all 20 active items before archive, all 18 remaining items after archive, and all 997 archived changes.
