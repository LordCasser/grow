# Fuzzing markdown

Coverage-guided fuzzing for the markdown renderer using [cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html) (libFuzzer).

## Prerequisites

```bash
cargo install cargo-fuzz   # if not already installed
rustup toolchain install nightly
```

## Targets

| Target | What it fuzzes |
|---|---|
| `render_all` | Four renderer calls: pretty and non-pretty full and streaming renders, all with Syntect disabled |

For each valid UTF-8 input, `render_all` runs:
- `render_markdown_ratatui_full()` with pretty enabled and disabled, passing `None` for Syntect.
- `StreamingMarkdownRenderer` with pretty enabled and disabled, also passing `None` for Syntect. It cycles through 1-, 16-, and 32-byte chunk targets, extending each chunk end to a UTF-8 character boundary before calling `push_and_render()`.

This target is crash/panic-oriented: it discards rendered output, does not call `finish()`, and does not compare full and streaming output or check structured properties. It does not exercise Syntect or ANSI-rendering paths.

## Running

From `crates/codegen/markdown`:

```bash
# Run indefinitely (Ctrl-C to stop):
cargo +nightly fuzz run render_all fuzz/corpus/render_all fuzz/seeds/render_all -- -max_len=16384

# Run for 5 minutes:
cargo +nightly fuzz run render_all fuzz/corpus/render_all fuzz/seeds/render_all -- -max_len=16384 -max_total_time=300
```

- `corpus/` — auto-generated inputs (gitignored)
- `seeds/` — hand-written seed inputs (checked in)

## Reproducing a crash

When a crash is found, the input is saved to `artifacts/render_all/crash-<hash>`. Reproduce it with:

```bash
cargo +nightly fuzz run render_all fuzz/artifacts/render_all/crash-<hash>
```

## Adding seed inputs

Drop `.txt` or `.md` files into `seeds/render_all/`. Good seeds cover distinct markdown features (tables, code blocks, emoji, nested lists, etc.) and help the fuzzer reach new code paths faster.
