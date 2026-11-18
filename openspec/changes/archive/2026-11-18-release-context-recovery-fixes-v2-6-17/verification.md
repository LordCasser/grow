# Verification

## Local candidate

- Last public release is v2.6.16. The remote v2.6.17 reference was absent at candidate preparation. The new workspace version, lockfile and changelog all identify v2.6.17.
- Pinned Rust 1.93.1. Full Shell, ChatState and Sampler libraries passed 5,853 tests in total, with 17 existing ignored tests. The scoped pressure, admission, cold storage and response usage groups passed separately. The independent architecture review and its real indexed-load regression are recorded with the context-recovery change.
- Rustfmt, default CLI check and build, workspace all-targets check, and release-dist profile/feature with `-D warnings` all passed. Existing test-only warnings remained visible in all-targets.

## Official gates

- Source commit `7d8f0773b728d7975e0610bcb3ca195bb4cdb154`, annotated tag object `3c173649b42ea40dd2ae5437cbd589fe5bb5989c`. Clean-tree `bash scripts/validate-release.sh v2.6.17` passed. The atomic push published matching main and peeled tag references.
- [Official Actions run 44271889829](https://github.com/LordCasser/grow/actions/runs/44271889829) succeeded on attempt 1. All 20 jobs passed, including ten platform builds, Core, three native coordination/export jobs, Windows storage, OpenSpec, and publication. The completed Core log reported 19,083 passing test results, zero failures, and 41 ignored across its 66 result groups, including precise ACP and PTY selections.

## Public verification

- [GitHub Release v2.6.17](https://github.com/LordCasser/grow/releases/tag/v2.6.17) published at `2026-11-18T10:13:44Z`. The API confirms latest, draft=false, prerelease=false, and an annotated tag pointing to the verified source. All ten archives and `SHA256SUMS` are present, and all ten release asset digests match the checksum file.
- Independently downloaded the macOS aarch64 archive. SHA256 `f6637242a5111f6684617ff4a864498ef5ee9931cdad7baca1dfbf29e5aa1601` matches both the API digest and the checksum file. Its single executable passed strict codesign verification and returned `grow 2.6.17 (7d8f077)` in an isolated HOME. `--help` succeeded. Artifact attestation verification bound the downloaded archive to the official build workflow and the full source commit.
