# Design

The source pixel gate remains before both conversion backends. `run_sips_command` installs `RLIMIT_FSIZE` in the Unix child before exec, lowering the inherited soft and hard limits only if needed. The private `output.png` cannot grow past 100,000,000 bytes even if `sips` writes faster than a parent-side poll could observe it. A file-limit signal or write failure is a failed converter status; the existing owned process-group cleanup and temporary-directory drop still run. The existing post-exit size/read checks remain as defense in depth.

The Rust fallback encodes RGBA pixels into a bounded `Write` adapter. It accepts an exact-fit output and returns `WriteZero` when the next write would exceed the limit, without extending its output vector. A failed encoder returns `None` through the existing overlay path. This bounds the encoded result buffer, not decoder-internal or system image memory; those are tracked separately.

Tests inject a small child file limit with a command that writes beyond it and verify the artifact remains at or below the limit; they also drive the PNG adapter at exact and overflow boundaries. Existing conversion and process-lifecycle tests guard normal behavior.
