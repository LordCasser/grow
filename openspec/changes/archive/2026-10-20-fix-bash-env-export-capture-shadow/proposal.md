## Why

Persistent Bash serialization declares locals before running `export -p`. Bash locals are dynamically scoped and shadow exported globals with the same name, so the snapshot can omit user values and replay can lose them. The previous serializer-isolation change explicitly left the `env_vars` collision unresolved; its allexport bookkeeping local has the same structural risk.

## What Changes

Capture the export listing before creating any serializer local that could shadow a user export. Keep the original allexport flag in the dump function's positional scope, disable allexport, and pass the listing directly to the existing encoder. Preserve exported user values through snapshot and replay. Add a production descriptor-path regression alongside a controlled source-script check. No shell authorization, transport, or unrelated serializer behavior changes.
