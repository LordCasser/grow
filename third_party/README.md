# Third-party vendored crates

This directory holds **upstream source** vendored into the repository. It is
**not** first-party application code.

## Why vendor

Some crates sit on the path that renders **untrusted model output** (diagram
source → SVG); others carry narrowly scoped release-target portability
patches not yet available in their published crates. Vendoring gives a full
audit surface and pins exact source. Local patches and upgrade checklists live
in each crate’s `Cargo.toml` header comments — treat those as the source of
truth when re-vendoring.

## Mermaid layout stack

| Crate | Version | License | Upstream | Full license text |
|-------|---------|---------|----------|-------------------|
| [`mermaid-to-svg`](./mermaid-to-svg/) | (path) | MIT | [warpdotdev/mermaid-to-svg](https://github.com/warpdotdev/mermaid-to-svg) | [`LICENSE`](./mermaid-to-svg/LICENSE) |
| [`dagre_rust`](./dagre_rust/) | 0.0.5 | Apache-2.0 | [r3alst/dagre-rust](https://github.com/r3alst/dagre-rust) / Warp re-vendor | [`LICENCE`](./dagre_rust/LICENCE) |
| [`nono`](./nono/) | 0.53.0 | Apache-2.0 | [always-further/nono](https://github.com/always-further/nono) | [`LICENSE`](./nono/LICENSE) |
| [`sqlite-vec`](./sqlite-vec/) | 0.1.10-alpha.4 | MIT OR Apache-2.0 | [asg017/sqlite-vec](https://github.com/asg017/sqlite-vec) | [`LICENSE-MIT`](./sqlite-vec/LICENSE-MIT), [`LICENSE-APACHE`](./sqlite-vec/LICENSE-APACHE) |

Dependency shape:

```text
mermaid
  └── mermaid-to-svg          (MIT)
        ├── dagre_rust        (Apache-2.0)
        │     ├── graphlib_rust =0.0.2  (crates.io)
        │     └── ordered_hashmap =0.0.3 (crates.io)
        └── graphlib_rust =0.0.2         (crates.io)

sandbox
  └── nono                    (Apache-2.0, riscv64 + musl patches)

memory
  └── sqlite-vec              (MIT OR Apache-2.0, packaging repair)
```

## Notices and ancestry

- **[`NOTICE`](./NOTICE)** — short index of the crates above (names, licenses,
  upstream links, paths to full text). Prefer that file for a one-page overview.
- **[`mermaid-to-svg/THIRD_PARTY_NOTICES`](./mermaid-to-svg/THIRD_PARTY_NOTICES)** —
  additional ancestry for the SVG engine (e.g. mermaid.js, dagre.js MIT notices).

British spelling **`LICENCE`** is intentional on the Apache crates (as upstream
vendored); grepping only for `LICENSE` will miss them.

## crates.io dependencies

Normal Cargo dependencies (tokio, serde, …) are **not** under `third_party/`.
They resolve via `Cargo.lock` / crates.io. Full attribution and license texts
for the Grow CLI dependency closure are maintained in
[`THIRD-PARTY-NOTICES`](../THIRD-PARTY-NOTICES).

This directory is only for **in-tree vendored** sources.

## Source tracking decisions

Submodules are preferred only when the current upstream repository can be used
unchanged. The audited dependency stack does not currently have such a member:

| Crate | Audited upstream baseline | Why it remains vendored |
|-------|---------------------------|--------------------------|
| `mermaid-to-svg` | `8d3f789c2eb49335d7bf247a06bb649f59b6d4ed` (current upstream `main`) | Grow carries parser, CJK sizing, wrapping, sequence/XY chart and hermetic-rendering changes. |
| `dagre_rust` | `mermaid-to-svg`'s copy at the same revision | Grow replaces an unsynchronized `static mut` id counter with `AtomicUsize`. |
| `nono` | 0.53.0 | This is the newest release compatible with the workspace's Rust 1.92 toolchain; Grow also carries riscv64 and musl portability patches. |
| `sqlite-vec` | 0.1.10-alpha.4 | The published crate omits four C files included by `sqlite-vec.c`; Grow adds the byte-identical files from the same tag. The repository root is not a Cargo package, so it cannot be consumed unchanged as a submodule. |

`graphlib_rust =0.0.2` and `ordered_hashmap =0.0.3` are exact crates.io
dependencies. Their former in-tree copies had no semantic patches and were
removed after verifying the registry sources against the Grow test suite.

The sqlite-vec vendor should be removed once upstream publishes a self-contained
crate that retains the musl portability fix.

## Upgrading

1. Read the `VENDORING NOTES` block at the top of the crate’s `Cargo.toml`.
2. Re-apply listed local patches (fmt, hermetic env, unsafe fixes, dropped bins/tests).
3. Confirm the license file still matches the declared `license =` field.
4. Refresh [`NOTICE`](./NOTICE) if versions or upstream URLs change.
