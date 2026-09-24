## Decision

OpenSpec specifies Grow's observable security boundary, not every vendored `nono` source symbol or manifest field. The archived 39 deltas were generated from static inspection and explicitly lacked Cargo, build-script and platform execution evidence. Do not adopt them wholesale or retain an unbounded task to validate an entire third-party implementation solely for possible spec expansion.

Keep the existing accepted sandbox requirement at its proven scope. A concrete divergence discovered in Grow's wrapper or a supported backend still warrants targeted threat testing and a new behavior change. The inherited connected-FD issue already has its own backlog entry and is not closed here.

## Verification

Compare the accepted `sandbox-boundary` spec with the archived audit and the current `sandbox` wrapper. Confirm this change removes only the broad inventory-watch entry, does not assert a new platform guarantee and leaves concrete sandbox debt intact. Run OpenSpec validation before and after archive.
