# Verification

- Publication run [36086806389](https://github.com/LordCasser/grow/actions/runs/36086806389) failed in Linux aarch64 before compilation: AlmaLinux 8 mirrorlist targets returned 404 for referenced BaseOS metadata; canceled the unusable run.
- AlmaLinux 8's bundled repo file lists the official `repo.almalinux.org/almalinux/$releasever/{BaseOS,AppStream,extras}/$basearch/os/` URLs and retains `gpgcheck=1`. Each aarch64 `repomd.xml` returned HTTP 200.
- The exact DNF repository override completed `makecache --refresh` and installed `gcc gcc-c++ make cmake git perl pkgconf util-linux` in an `almalinux:8` aarch64 container; `gcc --version` reported 8.5.0.
- Ruby parsed `.github/workflows/build-one.yml`; `git diff --check` passed; `openspec validate --all --strict --no-interactive` passed (15 items).
- Full release rerun and public-asset verification: pending.
