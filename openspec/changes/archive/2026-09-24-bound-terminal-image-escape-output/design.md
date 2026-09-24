## Context

Image conversion already limits source pixels and converted output bytes. The terminal path is separate: `kitty_chunked_escape` and `render_iterm2_image` currently encode the entire source into a temporary base64 `String`, then build another `String`; Kitty additionally allocates a vector of chunk references and formatted temporary strings. The image data must remain valid while these builders run, so an output bound and removal of full-size encoding copies directly address this Grow-owned staging boundary.

## Goals / Non-Goals

**Goals:**
- Enforce the byte ceiling against the exact complete escape output before returning it.
- Apply the same ceiling to each buffered inline-media accumulator across placements, cleanup escapes, subagent trees, and dashboard agents.
- Return failure atomically if the bound cannot be met or output allocation fails.
- Write base64 directly into a single bounded output `String`.

**Non-Goals:**
- Bound terminal emulator decode, GPU, or cache memory.
- Change image source/conversion limits or terminal protocol semantics.

## Decisions

- Use a 100,000,000-byte serialized-output limit, matching the existing conversion artifact ceiling while treating protocol expansion and framing as part of the output budget.
- Treat a draw accumulator and a takeover/cleanup accumulator as separate logical buffers, each with the same strict ceiling. Within a buffer, pass one remaining budget through all placements and recursive clear state. Dashboard stale clears reserve popup inline-media bytes before any state is drained, so their eventual merged inline-media post-flush buffer stays within the ceiling. Fixed-size clears written directly to stderr and unrelated notification escapes are separate output paths.
- Compute encoded length and exact framing overhead with checked arithmetic before allocating the output. For Kitty, encode source slices sized to produce at most one protocol chunk and append framing directly; for iTerm2, reserve the checked complete length and append base64 directly to the output string.
- Make upload builders return `Option<String>` so overflow or fallible reservation cannot be mistaken for a valid empty/partial escape. Propagate `None` through image callers.
- Count placement and clear escapes in the same inline-media frame aggregate. One accumulator and remaining byte budget flow across an agent's own placements, descendants, and dashboard agents. Remove IDs and iTerm2 emission state only after their corresponding clear escape is appended; retain clear work that did not fit for a later frame. Dashboard aggregation reserves space for the popup's frame output before draining stale IDs, keeping the merged output within the same ceiling.
- Keep fixed-size placement/clear escapes unchanged; their size is independent of image payload.

## Risks / Trade-offs

- Images previously accepted by source/conversion bounds may now be omitted if base64 plus protocol framing exceeds 100,000,000 bytes → callers retain their existing no-inline-image fallback behavior.
- A byte limit is not an RSS guarantee because allocator capacity and source image ownership are separate → describe the contract only as a bound on serialized Grow-owned escape buffers.

## Migration Plan

No persisted data or configuration changes. Update callers to handle builder failure, run focused image tests and OpenSpec validation.
