# Verification

- OHOS-only release runs [36020065845](https://github.com/LordCasser/grow/actions/runs/36020065845) and [36021539972](https://github.com/LordCasser/grow/actions/runs/36021539972) failed while Harmonybrew downloaded missing bottles. The latter still poured `cmake` and `patchelf` bottles although the requested formulas used `--build-from-source`.
- The third OHOS-only run [36025406330](https://github.com/LordCasser/grow/actions/runs/36025406330) got past Homebrew bootstrap and was compiling Grow when it was canceled; it was not an OHOS bootstrap failure.
- In the local `harmonybrew/ci-runner:latest` image (Homebrew `6.0.6_10`), checkout of pinned core `e3a9ec87f881ce05d563912f5f0cbd6f1693b4f3` and `HOMEBREW_NO_INSTALL_FROM_API=1 brew deps --include-build --include-implicit --topological rust` succeeded. The result includes `xz`, `bzip2`, `patchelf`, `ca-certificates`, `openssl@3`, `ncurses`, `cmake`, `zlib-ng-compat`, `unzip`, `ohos-sdk`, and `llvm-gcc-compat` in dependency order.
- `bash -n scripts/build-ohos.sh`, `git diff --check`, `cargo metadata --locked --offline --no-deps --format-version 1`, and `openspec validate fallback-ohos-bottle-build --strict --no-interactive` passed after the correction.
- The full OHOS release job for the corrected bootstrap is still pending. The change remains active until that result is recorded.
