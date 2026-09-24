## Approach

When the expected OHOS Rust toolchain is absent, install the pinned `ca-certificates`, `cmake`, `ncurses`, `openssl@3`, `zlib-ng-compat`, `bzip2`, `unzip`, `ohos-sdk`, `llvm-gcc-compat`, `xz`, `patchelf`, and `rust` formulas with Homebrew's source-build option. This avoids fetching OHOS bottles that may be missing from the mirror, including the CMake and patchelf build dependencies. Rust's formula source input is the official `aarch64-unknown-linux-ohos` Rust distribution; the formula still applies the required OpenSSL and zlib rpaths.

The existing core commit pin, toolchain version assertion, and downstream build configuration remain authoritative. Other formula dependencies continue to use their normal bottle resolution.
