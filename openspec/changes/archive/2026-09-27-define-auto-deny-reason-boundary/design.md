## Context

`ClassifierOutcome` keeps a 240-character structured reason so validation and diagnostics can inspect it. The manager currently builds fixed `PolicyDeny` text for a Block verdict. The two legacy tests instead require the classifier's prose in that tool result. Child UI audit already requires harness-owned decision reasons.

## Decision

Keep the fixed denial guidance for primary and child callers. Treat the model's reason as untrusted classifier evidence, not as an authorization message or instruction to the calling Agent. Audit projections retain canonical decision reason/source/verdict and do not interpolate model prose. Correct only the stale assertions and add an adversarial reason case. No new sanitizer or output channel is needed.

## Verification

Run the two previously failing manager tests and the full workspace permission suite; check Shell permission audit tests and OpenSpec validation.
