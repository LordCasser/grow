## Context

`load_skill_content` reads file-backed bodies for slash invocation and agent preloading. `load_skill_with_body` reads files while workflow capture freezes skill contents. Both currently use unbounded `read_to_string`; prompt inline budgeting happens after these reads. `load_skill_content` already treats an in-memory body, including an empty one, as an authoritative snapshot.

## Goals / Non-Goals

**Goals:**

- Bound bytes consumed and allocated by both file-backed loaders.
- Preserve complete-body semantics below the limit and return errors for oversized files.
- Leave caller error handling and in-memory snapshots unchanged.

**Non-Goals:**

- Capping aggregate bytes across multiple skills or bodies already supplied in memory.
- Changing skill discovery preview/frontmatter limits or caller policies.

## Decisions

- Use one private asynchronous helper for both loaders. It opens the file, reads at most `1 MiB + 1` bytes, rejects a probe byte beyond the cap, and decodes UTF-8 only after the size check. This keeps the source-consumption bound aligned across invocation and workflow capture.
- Count the whole source file, including frontmatter. The cap is a resource boundary for explicit body loading, not a body-text truncation budget.
- Keep errors as ordinary loader errors containing the source path and 1 MiB limit. Existing callers already fail or omit the affected skill on load error, so no partial body can enter prompt state.
- Keep preloaded snapshots ahead of file I/O in `load_skill_content`; workflow capture continues to read disk as its existing freeze operation does.

## Risks / Trade-offs

- A legitimate unusually large skill will now fail to load. The 1 MiB cap leaves ample room for normal instruction files while bounding a single source read; callers surface the specific failure.
- Separate individually valid files can still produce a large aggregate prompt. Aggregate prompt budgeting remains a distinct concern and is outside this change.

## Migration Plan

No data migration is required. Files at or below the limit keep existing behavior; oversized file-backed reads fail explicitly. Rollback consists of reverting the loader helper and this requirement.
