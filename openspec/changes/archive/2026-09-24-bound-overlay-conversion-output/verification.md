# Verification

- `cargo test --locked --offline -p pager-render --lib terminal::image::tests`: 24 passed. The child regression writes 2,048 bytes against an injected 1,024-byte file-size limit and verifies failure and an artifact no larger than the limit. The Rust regression accepts exact-size PNG output, rejects one byte less, and decodes the accepted result.
- An initial test showed that `image`'s PNG writer can report success while its drop-time final write overflows the bounded adapter. The adapter now records any rejected write, and the conversion returns `None` even if that error is swallowed by the encoder. The rerun above passed.
- `cargo fmt --all -- --check`, `git diff --check`, and `openspec validate bound-overlay-conversion-output --strict --no-interactive`: passed.

The result buffer and `sips` output file are bounded; decoder-internal and terminal-side allocations remain under their separately documented boundaries. No `sips` EPERM failure was observed in this change.
