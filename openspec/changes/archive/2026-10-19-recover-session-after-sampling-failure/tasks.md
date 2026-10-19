## Contract and implementation

- [x] Trace current sampler exhaustion, Turn terminal settlement, child output accounting and Goal restart behavior.
- [x] Define the recovery boundaries and acceptance scenarios before implementation.
- [x] Add production actor tests for three malformed completed attempts followed by manual continuation in the same live session across all three backends, without manually resetting foreground state.
- [x] Cover new input admitted during failure settlement and malformed protocol failure followed by manual continuation.
- [x] Verify known, exhausted and unknown child output budgets, including an unconfirmed sampler terminal; apply the minimal accounting fix established by the tests.
- [x] Verify Goal failure pauses, manual interaction remains usable, and explicit restart respects the original Goal identity and budget gates.
- [x] Update developer guidance with the recovery contract and focused test entry point.

## Verification

- [x] Demonstrate any confirmed defect with a failing regression before its fix.
- [x] Run focused recovery and adjacent sampler/accounting/Goal regressions.
- [x] Run formatting, diff checks and strict all-change OpenSpec validation; record exact results and limitations.
- [x] Confirm the patch contains no MCP changes and clean this task's Cargo compilation artifacts.
