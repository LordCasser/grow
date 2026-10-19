# Verification

## Confirmed hosted failure

Ubuntu Core run [39268899482](https://github.com/LordCasser/grow/actions/runs/39268899482) on 1886dd76 completed with 2422 tools tests passed, one failed and seven ignored. The existing set-a test failed before its user-command export assertion, with exit 126 and stderr `/usr/bin/tr: Argument list too long` and `/usr/bin/base64: Argument list too long`. All sixteen earlier grep failures disappeared after the separate ripgrep prerequisite fix.

## Reproduction and fix

A controlled Bash subprocess used only `/usr/bin:/bin`, an isolated temporary HOME and a 1,200,000-character function literal. The old source script completed two dump/replay rounds with allexport off, but failed its first dump with allexport on and the same argument-list diagnostics. The new script completed both rounds with either flag, preserving the callable function, exported user value and original flag. This source-script probe complements rather than replaces the real fd regression.

The serializer captures allexport before declaring payload locals, disables automatic export during capture, and explicitly removes local export attributes and emits only the original allexport flag after the other blocks, retaining the original POSIX/shopt ordering. Existing fd transport, dump parsing, command authorization, Zsh script and user exit-status handling are unchanged.

## Actual Rust path

Pinned Rust 1.93.1, `CARGO_TARGET_DIR=/tmp/grow-241-release-fix-target`:

`cargo test --locked -p tools --lib computer::local::shell_state::tests -- --nocapture`: **28 passed, zero failed/ignored**, including the new large-function test and existing allexport, failure-state, Bash/Zsh, aliases, functions and positional-argument regressions. The new test starts with a large function in the snapshot, transfers it through production prepare_command/fd 3/fd 4, and checks two replay rounds with allexport on and off. It verifies the large definition remains intact, callable and accompanied by the original exported sentinel and flag. No manual replacement of snapshots or smaller payload is used between rounds. Build output contains no compiler warnings or errors.

Hosted confirmation of the corrected final source remains a publication gate; the diagnostic failed run above is not reported as green. Release preparation records the final candidate and publication results separately.


## Adversarial boundaries

Review reproduced two issues in the first implementation before committing: inherited export attributes on same-name payload locals, and POSIX mode resetting explicitly disabled shopt values when restoring the whole POSIX block last. The final implementation uses `local +x` and delays only allexport. Controlled tests passed on system Bash 3.2 and Homebrew Bash 5.3 with exported `all_functions`/`content` collisions, both original allexport flags and two large-function replay rounds. Bash 5.3 retained `posix=on + inherit_errexit=off`.

The final Rust suite includes exported-name collision assertions and a separate real-fd POSIX/shopt regression. Bash before 4.4 lacks inherit_errexit, so only that option-specific test returns early on an unsupported Bash; source-script coverage on Bash 5.3 is explicit above. The large-function test does not depend on that option and runs on both versions. The pre-existing `env_vars` capture-local shadowing is recorded in backlog and is not claimed fixed here.
