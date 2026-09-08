## Why
Search bootstrap opens every Timeline ledger before its four-way semaphore. Each reader owns a file, so enough sessions exhaust process descriptors before indexing starts. Per-session timeouts can also release async permits while blocking reads still own their files.

## What Changes
Acquire bounded capacity before opening each identity-bound reader, retain capacity through blocking reads after timeout, and preserve existing canonical Timeline validation, title-only size handling and bootstrap ownership fences.
