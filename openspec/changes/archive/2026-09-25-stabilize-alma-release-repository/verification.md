# Verification

- Publication run [36086806389](https://github.com/LordCasser/grow/actions/runs/36086806389) failed in Linux aarch64 before compilation: AlmaLinux 8 mirrorlist targets returned 404 for referenced BaseOS metadata; canceled the unusable run.
- AlmaLinux 8's bundled repo file lists the official `repo.almalinux.org/almalinux/$releasever/{BaseOS,AppStream,extras}/$basearch/os/` URLs and retains `gpgcheck=1`. Each aarch64 `repomd.xml` returned HTTP 200.
- The exact DNF repository override completed `makecache --refresh` and installed `gcc gcc-c++ make cmake git perl pkgconf util-linux` in an `almalinux:8` aarch64 container; `gcc --version` reported 8.5.0.
- Ruby parsed `.github/workflows/build-one.yml`; `git diff --check` passed; `openspec validate --all --strict --no-interactive` passed (15 items).
- Full publication run [36088426966](https://github.com/LordCasser/grow/actions/runs/36088426966) succeeded: all ten platform build jobs and `Publish release assets` completed successfully.
- [v2.2.0](https://github.com/LordCasser/grow/releases/tag/v2.2.0) was published on 2026-09-25 at 04:04:31 UTC as a non-draft, non-prerelease Release. All ten expected archives are uploaded and nonempty, plus `SHA256SUMS`.
- Downloaded the published `SHA256SUMS` independently; it contains exactly ten 64-hex SHA-256 entries, one for every required platform archive. The publication job verified remote artifacts, attestations, checksum contents, and the unchanged annotated tag before making the Release public.
