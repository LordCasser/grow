## Why
Direct-exec auth helpers resolve a relative program by joining it to configured cwd. If cwd is itself relative, the joined executable remains relative and the child then changes to cwd, applying that prefix twice. Existing execution tests only cover absolute cwd.

## What Changes
Resolve a configured helper cwd to an absolute path before using it for executable resolution and child current_dir. Preserve shell commands, PATH-only executable lookup, and no-cwd behavior.
