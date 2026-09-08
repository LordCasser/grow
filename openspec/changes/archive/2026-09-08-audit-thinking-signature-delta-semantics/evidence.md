# Evidence (checked 2026-09-08)

## Local
- sampler/src/stream/messages.rs assigns `state.signature = signature` for a signature delta. It emits the completed block's signature at block stop and stores it in the corresponding native block.
- sampler/src/stream/messages_tests.rs covers per-block signature ordering for multiple thinking blocks. This does not establish multiple-delta assembly for one block.
- Previous live Grok evidence, recorded in recover-proxy-thinking-signature-mismatches, contains one signature_delta after an unsigned thinking start. No multi-fragment capture exists in this audit.
- The previous unsigned-final-response guard discards native continuation, while retaining visible facts. No changes made to that guard.

## Primary external sources
- [Anthropic Python SDK streaming accumulator](https://github.com/anthropics/anthropic-sdk-python/blob/main/src/anthropic/lib/streaming/_messages.py): assigns the event signature to the thinking block.
- [Anthropic Java SDK MessageAccumulator](https://github.com/anthropics/anthropic-sdk-java/blob/main/anthropic-java-core/src/main/kotlin/com/anthropic/helpers/MessageAccumulator.kt): mergeSignatureDelta explicitly uses replacement and explains its expectation of one signature event per thinking block.
- [Anthropic Go SDK message accumulator](https://github.com/anthropics/anthropic-sdk-go/blob/main/messageutil.go): appends the delta signature. This conflicts with the other SDK behavior; it does not prove that all proxy routes fragment signatures.
- [Claude streaming documentation](https://platform.claude.com/docs/en/build-with-claude/streaming): describes the signature event near block completion. The audit did not find a sufficient common guarantee to select concatenation for all supported routes.

These are source snapshots inspected on the date above, not pinned immutable SDK releases. Neither current assignment nor universal concatenation has been proven across arbitrary proxy implementations.
