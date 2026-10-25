## Reproduction and evidence

The reported session logged an ordinary provider request with `prompt_tokens=530111` immediately before the failed skill turn, followed by an ordinary request with `prompt_tokens=531346`. The UI's approximately 53% reading was consistent with those provider observations.

Before the fix, both new publication regressions returned `RejectedHeadroom` where a 53%-used projected request with omitted historical reasoning should fit. After switching the two commit checks to the existing ChatState projection, `cargo test --locked -p chat-state --lib skill -- --nocapture` passed all 12 tests.

The new tests cover both user and model publication. They verify complete guidance and a single token delta on acceptance, rejection when provider-anchored pressure exhausts the reserved budget without changing Surface revision, no partial user consumption, and the existing paired model error result.

## Checks

- The freshly built Shell test artifact passed `complete_request_headroom_rejects_user_skill_group_before_sampling` and `user_skill_pipeline_consumes_input_with_body_and_keeps_prompt_text`, both run by exact name with the documented `RUST_MIN_STACK=16777216`. An initial invocation without that fixture stack setting aborted with stack overflow; the documented setting resolved it.
- `cargo fmt --all -- --check` passed.
- `git diff --check` passed.
- `openspec validate --all --strict --no-interactive` passed (17 items before archival).
- After `openspec archive fix-skill-headroom-token-projection --yes`, full validation passed (16 items) and archived task validation passed (708 items).
