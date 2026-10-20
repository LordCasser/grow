# Verification — 2026-10-20

## Root cause and minimum repair

Baseline HEAD is 99556e83. vec0_vtab explicitly owns shadowMetadataChunksNames; vec0_init zero-initializes the owner and allocates one sqlite3_mprintf name for each metadata column, regardless of text/integer/boolean kind. vec0_free already releases column definitions but omitted these shadow names. The preceding 40-byte text case was one instance of this shared ownership omission.

The repair adds exactly two production C lines to the existing metadata loop: sqlite3_free the owned shadow name and set the slot to NULL. xDisconnect, xDestroy and constructor errors already call this destructor. The initialized count and NULL slots cover partial allocation. vec0_free_resources stays unchanged: xSync and rename must finalize statements while retaining live table identity. Rename already frees each old name before replacing it; destruction now releases the final generation. No allocator framework, schema/query change or unrelated error-path redesign.

Scoped Atlas search resolves vec0_free in third_party/sqlite-vec/sqlite-vec.c; the lifecycle query did not return within the interactive budget and was stopped, so no lifecycle result is relied upon. Manual review of all field references and a separate read-only reviewer confirm the ownership paths above.

## Meaningful regression / red-green evidence

The existing native harness still exercises vector dispatch failures, failure recovery, supported float/int8/bit queries and text IN. Added lifecycle cases use actual SQLite APIs:

- Three metadata kinds in one virtual table; transaction commit followed by filtered search exercises statement cleanup with live names.
- ALTER TABLE RENAME, then another insert and filtered search verifies replacement names remain usable.
- sqlite3_backup copies to a fresh in-memory connection; registered vec0 xConnect reconstructs the owner, a filtered search uses it, and close exercises xDisconnect. The original table is then dropped through xDestroy.
- A pre-existing broken_info shadow table causes real xCreate to fail after shadow names have been allocated. A later valid construction/query verifies the same connection remains usable.
- A zero-initialized partial owner with two allocated names and one NULL slot is cleaned twice; this models partial allocation without nondeterministic OOM timing. It is not a claim of exhaustive allocator-fault injection.

Linux environment: Ubuntu 24.04 aarch64, GCC 13.3.0, libc 2.39, system SQLite 3.45.1. The exact same extended harness runs against either the frozen baseline C or the patched production source:

- Baseline with ASan/UBSan/LSan: exits 1, `SUMMARY: AddressSanitizer: 576 byte(s) leaked in 15 allocation(s).`
- Patched full harness with `ASAN_OPTIONS=detect_leaks=1:halt_on_error=1 UBSAN_OPTIONS=halt_on_error=1`: exits 0, all SQL assertions pass, no sanitizer diagnostics.
- Linux GCC `-O2 -Wall -Wextra -Werror -DSQLITE_CORE`: passes compile and full harness.
- Native macOS AppleClang with the same strict optimized warning flags and Homebrew SQLite: passes compile and full harness. Linux is authoritative for leak detection; macOS is not presented as a LeakSanitizer run.

The existing Core gate now runs this **entire harness** under ASan/UBSan and leak detection, removing --boundary-only and its early-return path. No leak suppression, disabled warning, ignored lifecycle case or optional-feature expansion.

Reproduce on Linux with libsqlite3-dev installed:

```sh
cc -O1 -g -Wall -Wextra -Werror -DSQLITE_CORE -fsanitize=address,undefined -fno-omit-frame-pointer third_party/sqlite-vec/tests/native_boundaries.c -lsqlite3 -lm -o /tmp/grow-vec-lifecycle
ASAN_OPTIONS=detect_leaks=1:halt_on_error=1 UBSAN_OPTIONS=halt_on_error=1 /tmp/grow-vec-lifecycle
```

## Scope / remaining limits

This validates the current enabled native implementation linked to system SQLite, not every upstream experimental feature, old SQLite build option, full Grow binary or platform. No broad Rust build is needed for two C destructor lines; no Rust source/dependency change. Historical archived diagnostic verification remains unchanged and correctly describes its earlier narrower coverage. No public release mutation, version change, push or MCP edit.

## Completion checks

- Separate read-only adversarial review confirms owner destruction versus statement cleanup separation, actual constructor-error timing, rename/reconnect coverage and the explicit partial-owner simulation limit; no further code change requested.
- Pre-archive strict validation: 17 passed. Change archived with one memory-search requirement added; post-archive current strict validation: **16 passed, 0 failed**. Archive validation: **645 passed, 0 failed**.
- YAML parse of modified Core workflow, git diff --check and updated evidence link resolution pass.
- Existing v2.4.1 tag object `2b2a19332ee90fdf5378a5bccaaf4f6e7e5df92a` and peeled source `5fcbd12b0c10afef1f34695394d2ad81ff5b5310` unchanged. Read-only public release API comparison against the preceding identity receipt confirms the release and all 11 assets retain their id/name/size/digest/updated_at.
- MCP branch `5b375b07c08ec113c06cb9fa426bd0353e114941`, stash, root Cargo.toml/Cargo.lock and active MCP change remain untouched. No commit, push or release operation in this repair turn.
- Temporary Linux container and its newly pulled image are removed after verification; pre-existing images are preserved. No Cargo target directory was created.
