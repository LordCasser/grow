# Close subagent observation boundary tests

## Why

The archived observation fixes have focused tests, but several cross-layer failure boundaries remain unverified: a real SessionActor wait competing with terminal completion and durable receipt acknowledgement, a source ChatState timeline query held beyond the public deadline, persisted-output reader saturation and panic, and the difference between an active child the model forgot to name and a retired child.

## Scope

Add test-only failure-injection coverage using existing SessionActor, ChannelBackend, coordinator and output-reader owners. Do not change runtime behavior. Keep this change at `skip_specs: true`; if a test exposes a production defect, stop and report the evidence for a separate behavior change.

The existing required Local agent coordination matrix runs the new observation/coordinator/cancellation groups on native macOS, Linux and Windows runners. This extends test execution only; it introduces no product platform switch or new workflow.
