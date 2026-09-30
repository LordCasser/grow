# Existing path

`grow/workflows/list` calls `WorkflowRegistry::scan`. The Registry appends the project `.grow/workflows` directory and the user `workflows` directory to one scan list, then independently reads and extends entries for each scope. Pager renders each returned entry unchanged. The same Registry also resolves Definitions by name and ID, so display-only deduplication would leave inconsistent execution identity.

# Decision

At directory discovery, compare the two computed paths first. For different path spellings in a trusted project, open both through the existing contained-directory boundary and compare their pinned filesystem identities. Omit the project scan only when paths or successfully opened directory entities coincide, then retain the user scan. If identity cannot be established, leave both paths to their existing scanners and diagnostics. The user directory owns the shared file regardless of the session's current working directory, and its User Definition ID remains stable. Keep scope-specific duplicate-name handling and priority unchanged when directories differ.

Test a session whose project root is the parent of the user `.grow` directory. On macOS temporary directories this also exercises the `/var` and `/private/var` aliases. Assert one listing, User scope, User Definition ID, and successful name resolution. Keep the existing distinct-path same-name test as evidence that cross-scope Definitions still coexist.
