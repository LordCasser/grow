## Why
Credential helper failures include a raw stderr excerpt in the returned error, which is subsequently logged. Serde token-output errors may also echo malformed stdout values. These streams can contain bearer material.

## What Changes
Keep helper output bytes out of failure messages while retaining exit status, JSON error category/position and stderr byte count. Preserve the existing bounded pipe draining and mint/cache failure behavior.
