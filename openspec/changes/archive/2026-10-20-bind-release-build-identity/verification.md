# Verification — 2026-10-20

## Root cause

The published OHOS smoke printed `grow 2.4.1 (unknown)`. CLI build.rs obtained identity solely through `git rev-parse --short HEAD`, discarded stderr and silently fell back; the release workflow's already validated immutable RELEASE_COMMIT stopped at container boundaries. Smoke printed or checked product version without checking source identity. Missing Git is not established: the release's actual OHOS image has /usr/bin/git.

Pulled the same image digest used by the release: `sha256:cc2b371ba6ec8f12e550cd658020183ae546eb7f1eb880f1f110149c778e4289`. In that image, root reading a repository owned by UID 1001 reproduces `fatal: detected dubious ownership in repository at '/tmp/foreign-checkout'`. OrbStack maps the host bind mount owner to root, so the direct host bind test alone would not reproduce this; the ownership-controlled repository probe does. Historical Git stderr was discarded, so this reproduces a concrete same-environment failure mechanism, not proof of the exact historical error text.

The minimal repair uses the existing validated full commit instead of trusting container Git or adding safe.directory exemptions. Build.rs validates exactly 40 ASCII hexadecimal characters, embeds seven lowercase characters, and tracks RELEASE_COMMIT environment changes in Cargo. Alma, cross and OHOS receive it explicitly; native jobs already inherit it. Exact native/Windows/OHOS smoke comparison rejects unknown or wrong identity. OHOS smoke requires nonempty GROW_VERSION when explicit source metadata is supplied. Local builds without that input retain the existing Git/fallback behavior.

## Executed checks

- `python3 -m unittest discover -s scripts/tests -v`: **23 tests OK, 1 skipped** on macOS (the OHOS execution case). The three new POSIX Cargo tests compile the actual production build.rs in an offline minimal crate, not a reimplementation. They verify valid uppercase input with Git unavailable, changed SHA using the same Cargo target, malformed/empty/non-Unicode rejection and actual local Git fallback.
- The skipped `OhosSmokeIdentityTests` was separately run in the actual OHOS image, **1 test passed, no skips**. It executes production scripts/build-ohos.sh --smoke with a minimal executable fixture, verifying correct identity passes, unknown fails, malformed commit fails, absent required version fails, and manual no-explicit-commit behavior remains. This tests shell gating, not a rebuilt Grow OHOS executable.
- bash syntax check for scripts/build-ohos.sh, Rust 1.93.1 rustfmt check for build.rs, Ruby YAML parse of both modified workflows, and git diff --check pass.
- Reviewed all build boundaries: explicit docker env for Alma/OHOS, CROSS_CONTAINER_OPTS for cross, inherited job env for native, exact seven-character source check in Unix and PowerShell smoke. Windows PowerShell gate has not been executed locally.
- CLI's separate local Git ref/HEAD cache tracking is not expanded into this release-input repair. Explicit environment invalidation is exercised with real Cargo caching.

Reproduce the Cargo checks with `RUSTUP_TOOLCHAIN=1.93.1 python3 -m unittest scripts.tests.test_release_build_identity.ReleaseBuildIdentityTests -v`. Execute the OHOS class in the above image mounted read-only at /workspace/grow with `PYTHONDONTWRITEBYTECODE=1`.

## Immutable release / scope

No build/release dispatch, tag rewrite, asset upload/replacement or version bump. Existing v2.4.1 remains historical: it has not retroactively acquired the repaired embedded identity. Root Cargo.lock and the in-progress MCP branch/worktree are outside the diff. Read-only before/after public identity comparison and final strict validation are recorded at completion below.

## Completion checks

- Strict all validation before archive: 18 passed. Both changes archived with their delta merged; strict all validation after archive: **16 passed, 0 failed** (15 specs and unchanged active MCP change). Archive validation: **644 passed, 0 failed**.
- Final Python discovery rerun: **23 tests OK, 1 OHOS-only skip on host**; that case separately passed in the actual OHOS image. YAML, shell/Rust formatting and git diff checks pass.
- Public release API before/after equality covers release id/status/date and all 11 assets' id, name, size, digest and updated_at. Remote/local tag object `2b2a19332ee90fdf5378a5bccaaf4f6e7e5df92a` and peeled source `5fcbd12b0c10afef1f34695394d2ad81ff5b5310` remain identical. MCP branch `5b375b07c08ec113c06cb9fa426bd0353e114941`, pre-existing stash and root Cargo.lock are unchanged. See [immutable identity receipt](../2026-10-20-bind-release-build-identity/immutable-release-check.json).
- Repair verification performed no commit/push/tag/release or hosted build. Subsequent user-authorized main commit preparation rechecked strict validation (16 current items and 644 archived changes), diff hygiene and unchanged public release identities. No publication or MCP change is included.
