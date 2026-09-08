## Why
The CLI Trace arm loads effective model config and constructs AgentConfig even though trace_cmd::run consumes neither. Invalid TOML can prevent collecting session evidence.

## What Changes
Remove this unused required config load from the Trace arm. Preserve common startup behavior and trace storage/output validation.
