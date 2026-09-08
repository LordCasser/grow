## Why
Following the proxy missing-signature fix, audit whether signature_delta should append rather than replace the accumulated signature. The event name alone is insufficient evidence for changing opaque cryptographic state.

## What Changes
Record primary-source differences and the current implementation boundary. No runtime behavior, protocol or specification changes; skip_specs is intentional for this source audit.
