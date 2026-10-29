## Verification

- `/tmp/grow-2.5.2-git-identity.log`: 4 passed. The tests use temporary real Git repositories and cover a normal checkout, a linked worktree, detached HEAD, packed refs transitioning back to a loose branch ref, and the nearest-existing-ancestor case for a nested packed branch after Git prunes its intermediate ref directory.
- Directly compiled `crates/codegen/pager/build.rs` and `crates/codegen/cli/build.rs` with `rustc`, then ran both with their `CARGO_MANIFEST_DIR`; each emitted paths under the repository's actual `.git` common directory rather than a crate-relative path. CLI still emitted the existing `RELEASE_COMMIT` rerun directive.
- `openspec validate fix-build-git-identity-invalidation --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: passed in the documentation closeout run (33 items).
- `git diff --check` on this change's build scripts, helper, and OpenSpec files: passed.
- Cargo was not run, per task scope.
