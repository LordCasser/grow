## Why
`/usage` reports totals and multi-model input/output, but lacks cache-hit percentages and per-model totals. Its production ledger key can use a provider-returned wire model instead of the selected provider/model, merging distinct routes.

## What Changes
Keep statistics in `/usage`. Show total tokens, input/output, cached input and cache-hit rate overall and for every provider/model, including a single model. Attribute each completed response to the catalog identity captured before sampling. Preserve the current explicitly labeled since-start-or-last-resume reporting window. Record historical reconstruction debt separately; do not change persistence or `/session-info`.
