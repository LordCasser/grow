## Why
Follow-up on sampling log credential fragments requires checking whether the independent 401 callback has a production sink. Repository search finds only a test implementation and configuration forwarding of default None.

## What Changes
Record removal candidate R32 with explicit boundaries; correct misleading callback comments claiming short credentials cannot cross the boundary. No runtime change or deletion. skip_specs: true because this is reachability audit and documentation only.
