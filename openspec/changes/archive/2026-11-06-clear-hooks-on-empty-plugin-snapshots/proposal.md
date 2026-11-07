# Clear hooks on empty plugin snapshots

## Why

Session plugin adoption stores a new None registry but removes prior `plugin/` Hook contributions only inside the Some branch. Removing the last discovered plugin can therefore leave its executable command/HTTP handlers active after the plugin list is empty (AC-02).

## What Changes

SessionActor replaces the plugin contribution to its HookRegistry on every adopted snapshot, including None. Config/file/Agent Hook contributions remain, Hook generation changes are published through the existing path, and empty snapshots retain existing MCP and SkillManager refresh behavior. No new registry, watcher or plugin installer behavior is added.
