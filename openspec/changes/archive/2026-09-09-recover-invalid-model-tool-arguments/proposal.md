## Why
A completed Responses output can contain malformed model-generated tool JSON. Treating this as a fatal generic serialization error pauses unattended Goals on a potentially recoverable generation failure.

## What Changes
Introduce a narrow typed invalid-tool-arguments failure after completion validation. Discard the entire unaccepted response and resample the unchanged request through the existing bounded retry, evidence and usage pipeline. Keep all other protocol validation strict.
