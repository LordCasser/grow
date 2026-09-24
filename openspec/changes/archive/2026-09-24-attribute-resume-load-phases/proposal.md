## Why

The new PTY baseline observes about 9 seconds until the last history marker becomes visible at 512 turns, while post-load key echo remains fast. The existing Shell benchmark measures the same production `session/load` path separately; matching its synthetic fixture to the PTY fixture can distinguish Shell replay work from Pager/terminal work before changing either path.

## What Changes

- Run the existing ignored Shell end-to-end load benchmark with the PTY fixture's turn, catalog, text, and rewind knobs at 128 and 512 turns.
- Record Shell phase timings beside the already archived PTY measurements and narrow the long-session backlog to the observed bottleneck.

No runtime behavior or contract changes occur. This is a measurement/analysis change with `skip_specs: true`.
