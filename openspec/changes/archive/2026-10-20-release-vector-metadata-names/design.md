## Ownership and minimum repair

vec0_init zero-initializes vec0_vtab, assigns numMetadataColumns before allocating shadowMetadataChunksNames, and enters vec0_free on later failure. vec0Disconnect and vec0Destroy use that same destructor. Add sqlite3_free plus NULL assignment beside metadata_columns[i].name cleanup; NULL entries from partial allocation are safe and repeat cleanup cannot double-free. Do not release owned names in vec0_free_resources: sync and rename use that helper while the virtual table remains live. vec0Rename already frees old names and replaces them; the destructor must release only the currently owned names.

The backlog label says text metadata because the first reproducer used text. Code allocates names for every metadata kind, so cover multiple metadata columns, not only text. Do not add an allocator abstraction or change unrelated initialization/error semantics.

## Verification

Use the existing actual-source C harness. Prove the current source fails complete SQL LeakSanitizer and the fixed source passes without disabling leak detection. Add real SQL create/insert/rename/query/drop, post-allocation constructor failure (shadow-table collision), connection close, and reconnect through a copied database on another SQLite connection. A direct zero-initialized owner with some NULL metadata entries exercises partial allocation/repeated cleanup. Keep valid MATCH/k and dispatch boundary checks. Core runs the whole harness under ASan/UBSan/LSan; remove the boundary-only escape rather than retaining a narrower green gate.

Review native source manually and with scoped Atlas search. Atlas function-local lifecycle extraction did not return within the interactive budget; it is not treated as proof. Linux sanitizer evidence is required because macOS lacks the same leak detector. No claim of full product or all optional sqlite-vec features coverage.
