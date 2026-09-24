## Approach

When the expected OHOS Rust toolchain is absent, install the pinned `ca-certificates`, `openssl@3`, `zlib-ng-compat`, `ohos-sdk`, `llvm-gcc-compat`, and `rust` formulas with Homebrew's source-build option. This avoids fetching OHOS bottles that may be missing from the mirror. Rust's formula source input is the official `aarch64-unknown-linux-ohos` Rust distribution; the formula still applies the required OpenSSL and zlib rpaths.

The existing core commit pin, toolchain version assertion, and downstream build configuration remain authoritative. Other formula dependencies continue to use their normal bottle resolution.
