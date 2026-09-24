# Verification

- Source review: `from_heights` collects all heights before prefix accumulation; both full and incremental paths use `u16` inputs and `usize` totals.
- Release target review: `.github/workflows/release.yml` builds x86_64, aarch64, and riscv64 Linux/macOS/Windows artifacts; no 32-bit artifact target is listed.
- Bound: on 64-bit, more than 2^48 rows each at `u16::MAX` would be required to exceed `usize::MAX`. A `Vec<u16>` of that length is not a realizable cache allocation. On 32-bit, the risk could be reached around 65,538 maximum-height rows, but 32-bit is unsupported by the release matrix.
- No runtime or behavior contract changes; no Cargo build or tests were needed.
