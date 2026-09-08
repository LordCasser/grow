## Why
client_post logs raw authentication-header prefixes and sampling_request spans log token tails. Short keys can be fully exposed in sampling.jsonl.

## What Changes
Remove credential fragments from these event/span fields while retaining auth type and presence metadata. Preserve request headers and the independent 401 attribution callback.
