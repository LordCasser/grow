# Design

Reuse `resume_input_latency.rs` and its synthetic 512-turn fixture. Append a visible marker to every replayed agent text chunk, wait until one is visible while the final replay marker is still absent, then type a draft and measure 20 successive key echoes with `GROW_TEST_FRAME_WRITE_DELAY_MS=40`. Require the final marker to remain absent through measurement, and after it appears with the resumed prompt, require the original draft to remain visible. Marking each chunk keeps the in-progress signal on screen even when early messages have scrolled out.

Keep the test ignored with the existing manual long-session PTY suite. No production code or user-facing contract changes are part of this work.
