## Design

`ScriptedResponse::into_response_paced` gates the last event in its supplied SSE vector. The Responses helper emits `response.completed` followed by `[DONE]`, while `sampler::stream::responses` accepts the completed response and breaks immediately on `ResponseCompleted`. Blocking the sentinel permits completion to race the PTY key input. The same mismatch also makes the fixture's parent spawn-A barrier ineffective.

For these two explicitly blocked Responses scripts, remove and assert the helper's trailing `[DONE]`. Their last event is then the unchanged semantic `response.completed`; text deltas remain visible before the barrier. Existing expectation release and cancellation behavior provides all synchronization. No production code, shared harness policy, timeout, retry or specification changes are required.

## Verification

Run the exact normal and Minimal notification recovery cases against the current CLI, checking that both preserve the original notification Turn and unfinished child through suspension and complete each once on cold resume. Preserve the failed CI evidence and record local results. Check formatting, diff and strict active/archive OpenSpec before archive; the next exact-source Core CI remains required before release.
