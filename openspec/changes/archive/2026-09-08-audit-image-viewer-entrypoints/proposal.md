## Why
Viewer background infrastructure does not by itself establish that real input uses it; constructor admission needs an explicit audit.
## What Changes
Read-only audit of viewer constructors, background load dispatch and production admission. No runtime contract changes; skip_specs applies. Record an unused synchronous path constructor as a removal candidate and separately plan the live synchronous-input bug.
