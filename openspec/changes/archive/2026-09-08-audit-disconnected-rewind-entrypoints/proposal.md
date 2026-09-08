## Why
Identify disconnected rewind entry points after tracing the shell-owned transaction path. A standalone workspace helper still describes itself as the shared local/ACP implementation despite having no repository caller.

## What Changes
Record R30 for the standalone helper and its exclusively associated response types, and R31 for two test-only tracker convenience methods. Correct the obsolete helper documentation. No runtime code is removed, enabled or changed; skip_specs is true because this is reachability audit and documentation only.
