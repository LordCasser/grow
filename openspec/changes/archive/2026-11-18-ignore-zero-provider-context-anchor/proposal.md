## Why

The reported session alternates metered Messages responses with complete tool-call responses carrying all-zero usage. Grow uses these zero totals as context observations, resetting the context indicator and the request-pressure projection for the same live conversation.

## What Changes

- Keep the current context projection when an admitted response reports zero total tokens.
- Continue settling provider-reported usage through the existing attempt ledger, independently of context calibration.
