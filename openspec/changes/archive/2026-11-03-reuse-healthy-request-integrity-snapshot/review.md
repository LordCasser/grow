# Independent review

Result: no actionable production or validation defect found. Review compared mutations.rs and request_builder.rs with the snapshot named by `/tmp/grow-request-snapshot-baseline-path.txt`, then inspected final tests and the preserved probe. The reviewer did not run Cargo or edit production/tests.

## Ownership and acknowledgement

- `ensure_conversation_integrity_durably` returns the existing candidate only when every repair report field is unchanged. The quarantine/normalize/dedup/strip/dangling pipeline was checked, including the preceding C2 editors: a false changed flag still means the complete candidate values are unchanged.
- A changed candidate follows the original durable replacement and returns None only after successful acknowledgement. The request builder then copies the committed Surface. No candidate is installed or returned on a failed/ambiguous commit. The buffered helper was not altered.
- The user-message caller discards the Option at the same statement; it does not retain a healthy request copy into its subsequent Timeline clone or add a post-repair clone.
- MemoryContext retains its trim and duplicate rules. The event contains a clone of the same typed item; the owned request vector receives that item only after commit succeeds. Messages/Append appends exactly its items. Neither prompt refresh nor context-pressure refresh mutates the authoritative Surface, and this event does not activate pending Control contexts. The serialized actor cannot admit another command between these steps.
- Goal, image and native-continuation projections remain in their prior order after both durable boundaries. Moving the snapshot earlier does not change item values or shared Arc identities. These request-only projections do not mutate the persisted Surface.

## Real request-stage tests

The new success fixture starts with an empty constructor and publicly acknowledges a well-formed Assistant declaring an unanswered call. It then cold-folds those raw events and checks that only the pending Assistant exists. The observed repair therefore occurs during build_request rather than startup repair.

The build remains pending while the repair ACK is held, after a failed ACK, and while the later memory ACK is held. The retry is compared as a complete event. After both successful acknowledgements, the test compares full request items against both canonical Surface and a cold Timeline fold, including the synthetic result and exactly one reminder.

The two cancellation cases cancel the actor token while repair or memory is unacknowledged and require an error instead of a request. Their cold checks deliberately use only the raw acknowledged prefix, or that prefix plus the acknowledged repair. They do not confuse MockPersistence attempts with committed facts. This verifies those confirmed prefixes; it does not assert that a real write whose ACK is unknown must be absent from disk. Existing uncertain-write recovery semantics remain unchanged.

The strengthened Goal test covers healthy candidate reuse with a new memory reminder, request-only obsolete-Goal shadowing, unchanged original canonical content, full cold-projection equality and no new event for a repeated reminder. Existing no-memory, tool, image, durable user-message and memory-retry tests remain in the owner-reported full suite.

## Measurement limits

The preserved probe compares the same repair function with two healthy copies versus one reusable copy. Fixture construction, complete serialized equality checks and sample sorting are outside timing. Each timed sample averages 16 operations, with two warmups and seven samples. Final returned Vec drops occur inside both timed loops; the old path also drops its discarded integrity copy. All 12 raw sample rows have correctly reported medians.

Text, paired tool-call/result and short image-Arc fixtures cover 128/512/2048/8192 items. This demonstrates the healthy clone-pass saving, not actor/persistence/Goal/image/provider request throughput. Arc payload bytes are shared rather than deep-copied. No C4 heap/RSS measurement was performed. In requests appending a new MemoryContext, the owned vector now spans the ACK and overlaps the commit's temporary context-pressure copy; lower peak memory for all requests is not established. This boundary is accurately disclosed in measurement.md.

## Owner-executed validation

After removing the temporary timing probe/helpers from the build, the owner reports chat-state **608 passed, 0 failed, 3 ignored**. Targeted request-time tests passed 2/2, Goal/memory coverage passed 1/1, and the explicit probe passed. verification.md records `--locked`, isolated GROW_HOME and RUST_MIN_STACK=16777216. These results were executed by the implementation/measurement owner, not this reviewer. The probe source remains outside the build with reproduction instructions; its later Markdown-to-Rust wrapper cleanup does not alter the measured code.
