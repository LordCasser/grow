## Why
Audit same-model provider route reload after the proxy signature fix. User selection and catalog reload have distinct activation paths; reading only user selection incorrectly suggests that unchanged IDs always preserve native state.

## What Changes
Add a regression exercising catalog reload with actual signed native context under the same ModelId and changed transport. No production behavior or contract changes; skip_specs is intentional for verification-only work.
