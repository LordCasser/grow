## Why
During bound-sips-output-reads validation, sips_runner_preserves_exit_status intermittently returned raw EPERM. No runner production behavior changed in that output-read slice. The current error lacks stage attribution, so spawn/detach, attachment and cleanup must be distinguished before fixing anything.
## Scope
Investigate and reproduce only; no contract change yet (skip_specs). Preserve the existing requirement to report cleanup failures. A discovered behavioral fix requires a delta before implementation.
