## Approach

When the expected OHOS Rust toolchain is absent, install the pinned `openssl@3` formula with Homebrew's source-build option, then install the pinned `rust` formula the same way. This avoids fetching their missing OHOS bottles. The Rust formula's source input is the official `aarch64-unknown-linux-ohos` Rust distribution; the formula still applies the required OpenSSL and zlib rpaths.

The existing core commit pin, toolchain version assertion, and downstream build configuration remain authoritative. Other formula dependencies continue to use their normal bottle resolution.
