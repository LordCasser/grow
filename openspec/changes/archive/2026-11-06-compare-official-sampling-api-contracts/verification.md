# Verification

## Scope and source identity

User requested official API/SDK research, then clarified that the main issue is switching models across protocols within one session. The investigation follows source response facts → route replacement → portable target-history projection → target acceptance. The earlier single-request decode findings are related evidence, not a complete explanation of model-switch behavior. No production code, public release, runtime configuration or user session was changed, and no real target endpoint was used for a verification request.

Main remained `8ab486a6e45870561b91dba872934a6e24a795f1`, four local commits ahead of published main. Public2.6.1 still peels to `deaca4899bdec260ddc92a7bc98ceaa8da0ee5bd`. RootAGENTS, OpenSpec index, model-sampling contract and the archived2026-10-29cross-route reasoning design/audit/verification were reviewed. No nestedAGENTS applies to these documentation paths.

Source hashes recorded by parent:

| File | SHA256 |
| --- | --- |
| sampling-types/src/conversation.rs | 544b134e2840e3e35c3e199f8b0d3e125da63d1881a52faeb715a13211d58b06 |
| chat-state/src/actor/state.rs | 9f86b65834b9c7f8ba58db4afc2ec82b1105e77da011e6a61516abaaad121e3a |
| chat-state/src/actor/mod.rs | fee2c994ea2dcce67815883a4bd8ff8ad1a5b1b10a340a061244369278625f9a |
| sampling-types/src/error.rs | fe2fc4c7fe0fdb7679f7a80e6d968d25aa57701b6d1753c8754fe140a0f7ebd6 |
| sampler/src/client.rs | 8516297cb9d1a922914d583cbb8207c2c789b942300fd2442d258468a76fcae5 |

Paths above are undercrates/codegen. Parent also read Shellmodel-switch publication and turn recovery, Messages/ChatL2 relevant accounting, native-projection conversion and current regression fixtures. Static source conclusions are not represented as new live-provider observations.

## Official evidence accepted

Parent directly downloaded fixed OpenAI OpenAPI, Python/Node model/runtime sources, and current official reasoning/conversation/tool guides. sources.md provides URLs, commits, required-list differences, key quotes and artifact manifests. Context7 was used for official-source navigation; several Fetch URLs returned404. Important conclusions were rechecked against direct official repository files or directly fetched Markdown/HTML, not solely search summaries.

Anthropic child report at `/tmp/grow-official-anthropic-3quxzBvb/REPORT.md` was accepted after parent read full report, errors/streaming originaltext, fixed SDKstream iterator and Grow classification. Error taxonomy and retry advice were kept separate: api_error has explicit exponential-backoff advice; timeout_error is a known type with potentially504/500status; HTTP200 stream iterator errors are not automatically replayed by the checked SDK path. Actualrouteversion andbetaheaders remainunknown. The parent independently fetched currentthinking.md and verified summarized/omittedthinking, unmodifiedtool-turn replay and compatible same-vendor model-switch rules.

Chat/DeepSeek child report at `/tmp/grow-official-chat-GcDkz73j/report.md` was accepted after parent read the complete report, originalmanifest, DeepSeekthinking/toolrestrictions and Chat tail/accounting sources. Parent separately checked OpenAPICompletionUsage and Chat streamusage nullability, officialSDKtoperror handling and currentChatprojection. The obsolete DeepSeekreasoning_modelURL returned semanticPageNotFound or a200homepagefallback; it was not used as the currentreasoning source. The direct thinking_mode andtool_calls bodies are retained with hashes.

SDKsource permits loose construction/JSON.parse; this does not establish a wire permission to omit required fields. OpenAI officialschema and SDK also disagree on some optional/default fields; these differences are recorded rather than silently resolved by imposing new strictness. Grow's lockedasync-openaifork is a communitydependency, not an officialOpenAI SDK.

## Current route/projection regressions

Parent ran fixed Rust/Cargo1.93.1, standardPATH, RUST_MIN_STACK=16777216, CARGO_BUILD_JOBS=2, `--locked --offline`, serial tests. Five targeted invocations completed in54.79s, exit0, with no source/test changes:

| Invocation filter | Executions | Result |
| --- | --- | --- |
| sampling-types --lib portable_ | 14 | passed |
| sampling-types --lib learned_ | 4 | passed |
| sampling-types --lib agent_message_projection | 4 | passed |
| chat-state --lib endpoint_switch_matrix_strips_native_reasoning_and_diagnostics | 1 | passed; six cross-backend directions and A→B→A are inside this test |
| chat-state --lib reasoning_replay_is_route_local_and_acknowledged | 1 | passed |

Total24 executions,22unique tests because two overlap filters. Logs are `/tmp/grow-official-chat-GcDkz73j/parent-{portable-tests,learned-tests,runtime-projection-tests,switch-matrix-test,route-replay-test}.log`. Results establish localpairing, nativeisolation, route-localACK, boundaryhandling andruntimeexpansion. They do not establish that first-partyDeepSeekaccepts clientsynthetictoolhistory orsummary/emptyreasoning after a foreign-modelswitch.

The archived2026-10-29real nine-stage/six-direction matrix remains valid for the routes it tested. It never received a reasoning400; therefore it is not evidence that400→enableportable→targetacceptance works under currentfirst-partyrequirements. The source-onlyVisibleReasoningtext cannot reconstruct originalfullreasoning that the source never exposed. Chatfinalexpansion atconversation.rs:4010alreadyfillsreasoning_content on runtimeassistants in learnedmode; the present risk is semanticacceptance/targetstructure, not a falselyclaimedmissingkey.

## Latest created_at status question

Parent re-read the completed sparsefailedchange and checked the actual commit/tag ancestry. Commit `8ab486a6e45870561b91dba872934a6e24a795f1` is `fix: preserve sparse Responses provider failures`, present in currentHEAD; no tag contains it. Previous accepted evidence includes a failing olddecoder realHTTP/SSEcase, a passing correctedcase,63clienttests and326serialSamplerlib/integrationtests. It preserves providererror/code/message for the exact observedsparsefailure, no synthesizedtimestamp/identity/usage orCompletedcandidate. This is a next-versioncodefix; the currentlypublished2.6.1doesnotcontainit and runningprocesses do not hot-load sourceedits. Other missingcreated_atshapes are not automatically proven fixed. Parent reran the current exact `client::tests::responses_sparse_failed_after_partial_tool_output_preserves_provider_error` under fixedRust1.93.1/locked/offline: exit0,1/1passed. Log: `/tmp/grow-official-chat-GcDkz73j/parent-created-at-current-recheck.log`. No fresh failing rawframe was available from the user's latest text, so no current-runtime or new-shape root cause is inferred.

## Review and documentation closure

Two official-source child deliveries were reviewed and accepted. Optional cross-route reviewers did not produce source-access evidence before cancellation and were explicitly released, not counted as independent review. One flash dispatch with unsupported medium effort was rejected before initialization and released; its default-effort retry also produced no source evidence before the newer Windows incident took priority. Cross-route conclusions are the primary architect's reviewed conclusions, supported by direct source/doc inspection and 22 unique current regressions; no independent acceptance is claimed for that optional check.

analysis.md leads with cross-protocolreasoning/native/runtimehistory, sources.md preserves primaryevidence, and backlog/originalresidualanalysis were updated to distinguish internalcontractdifferences from officialguarantees. This is a skip_specspuredocumentationchange: no requirementswere silentlychanged, no productiontestsmerged, no Gitcommit/push ornewreleaseperformed. Final checks passed: `openspec validate --all --strict --no-interactive`16/16, `openspec validate --archived --no-interactive`871/871 and `git diff --check`. Logs are `/tmp/grow-official-chat-GcDkz73j/{specs-final,archives-final}.log`; archive links to the preceding analysis and backlog now resolve. The newer Windows result-delivery report is a separate investigation, not an uncompleted part of this source comparison.
