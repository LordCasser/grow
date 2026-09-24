## Approach

When the expected OHOS Rust toolchain is absent, resolve the pinned Rust formula's required, build, and implicit dependency closure with `brew deps --include-build --include-implicit --topological rust`. Install each dependency separately with `--build-from-source`, then install Rust the same way. Homebrew's `--build-from-source` applies to the requested formula but can still pour its dependencies from bottles, so passing the whole list to one `brew install` does not guarantee a source-only closure. Force local formula resolution after the core checkout. Rust's formula source input is the official `aarch64-unknown-linux-ohos` Rust distribution; the formula still applies the required OpenSSL and zlib rpaths.

The existing core commit pin, toolchain version assertion, and downstream build configuration remain authoritative. A future pinned formula dependency is included automatically, without another hard-coded name list.
