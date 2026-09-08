## Evidence
SlashController record_command_use touches the shared per-App store, takes an owned snapshot and queues persist_async. Its OnceLock writer only coordinates this process. MruSnapshot::write constructs one fixed path via with_extension("json.tmp"), then creates, syncs and renames it; failures remove that shared path.
## Design
Use the existing tempfile dependency to create a uniquely named file beside the destination. Write and sync the owned handle before persist replacement. Tempfile ownership handles failures without deleting another writer's path. Keep store format, recency calculation, serialization, queue behavior and persistence retry semantics unchanged.
## Limits
This ensures complete snapshot publication, not merging snapshots across processes, directory-fsync crash durability or guaranteed delivery at application shutdown. Those are separate semantics.
