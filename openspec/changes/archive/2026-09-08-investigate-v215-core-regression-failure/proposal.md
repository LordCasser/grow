## Why
Resolve milestone CI failures. Requested as part of the second cleanup batch before republishing v2.1.5.

## What Changes
Core run 34213624575 at 03eb765a failed: three builtin extraction tests failed before shell test process aborted from stack overflow. Preserve failure evidence and isolate each cause; do not mask by disabling tests or blindly increasing stack sizes. Release is paused until all newly requested work and verification complete.
