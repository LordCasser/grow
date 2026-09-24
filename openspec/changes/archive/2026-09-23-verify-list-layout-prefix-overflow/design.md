# Design: Verify list layout prefix-sum overflow boundary

`from_heights` first collects all `u16` heights into a `Vec`, then accumulates them in `usize`; `extend_heights` only appends to an existing Vec-backed cache. On supported 64-bit release targets, reaching `usize::MAX` would require over 2^48 rows at the maximum height of 65,535, far beyond a realizable cache allocation. The release target matrix contains x86_64, aarch64, and riscv64 artifacts, with no 32-bit target. A 32-bit build could reach the bound with tens of thousands of rows, but it is outside the supported artifact matrix.

Do not add saturating arithmetic: it would conceal an impossible supported-target overflow by silently assigning duplicate, inaccurate row positions. Keep the unsupported-target observation out of the active product backlog.
