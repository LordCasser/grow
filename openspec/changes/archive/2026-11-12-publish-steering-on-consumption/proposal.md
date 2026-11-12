## Why

In session 01a1c301-0971-7f20-af72-29e37f07151b, two pending steers appeared twice after cancelling their target's foreground tool. Timeline sequences 21667–21702 show one submission and one consumption per input. Both inputs were routed back to FIFO on cancellation. The live transcript had already rendered each steer on submission or admission and rendered it again when the original input reached its actual delivery boundary. The PTY reproduction also exposed a stale queue submission restored as the cancelled foreground prompt in the composer.

## What Changes

- Show steering input in the live transcript when consumption is durable, including a queued row promoted into a Turn and a local queued row sent through direct steering.
- Use the persisted display message identity for live delivery. An unconsumed input continues through the existing FIFO fallback with one eventual transcript row.
- Restrict pristine cancellation rewind to a stash that matches the cancelled foreground prompt.
