## Context

`fuzz_targets/render_all.rs` rejects invalid UTF-8, invokes the full renderer twice with `syntect=None`, then invokes streaming twice with `syntect=None`. Streaming chunks cycle through byte targets 1, 16, and 32 and advance each end to a UTF-8 boundary. Render outputs are discarded and `finish` is not called.

## Goals / Non-Goals

**Goals:** Align the user-facing README and manifest summary with this implementation and retain the existing run and crash-reproduction commands.

**Non-Goals:** Change the fuzz target, add a differential/property oracle, add Syntect or ANSI coverage, or run a fuzz campaign.

## Decisions

Keep the target name and invocation shape. State the four actual renderer calls and explicitly disclose that outputs are discarded, so users can distinguish crash/panic exploration from equivalence or property testing. Correct both active descriptions; historical audit records remain unchanged.

## Risks / Trade-offs

- The corrected documentation makes the target's current limits more visible but does not improve coverage → Record differential/property checks and Syntect/ANSI paths as future work; do not imply they ran.
