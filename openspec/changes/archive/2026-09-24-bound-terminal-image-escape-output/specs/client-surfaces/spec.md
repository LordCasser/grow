## ADDED Requirements

### Requirement: Terminal image escape buffers have a strict output budget
Grow SHALL construct each iTerm2 or Kitty image-upload escape buffer, and each buffered inline-media draw or clear accumulator, with no more than 100,000,000 serialized bytes, including protocol headers and chunk framing. Accumulators SHALL share that budget across placements, obsolete-ID clears, and recursively drained agent/subagent clear state; dashboard stale clears SHALL also reserve space for the popup inline-media output they precede. Grow SHALL reject an image upload whose complete escape output does not fit and SHALL NOT return a partial upload sequence. Grow SHALL encode into the bounded output buffer without first materializing a full-size base64 string. This budget covers Grow-owned escape output only and SHALL NOT be described as limiting terminal-process decode or cache memory. Fixed-size modal/subsession clears written directly to stderr and unrelated notification escapes are separate output paths, not members of the inline-media buffer budget.

#### Scenario: Kitty upload fits the output budget
- **WHEN** the complete Kitty upload escape sequence is at most 100,000,000 bytes
- **THEN** Grow returns the complete sequence with its existing chunk framing and transmission semantics

#### Scenario: Image upload exceeds the output budget
- **WHEN** Kitty or iTerm2 framing plus encoded image data would exceed 100,000,000 bytes
- **THEN** Grow returns no image upload sequence and does not retain or expose a partial sequence

#### Scenario: Buffered inline-media accumulator exceeds the output budget
- **WHEN** appending another complete placement or clear escape would make its buffered inline-media accumulator exceed 100,000,000 bytes
- **THEN** Grow omits that escape atomically and keeps the accumulator within the limit

#### Scenario: An old Kitty image clear does not fit
- **WHEN** the current frame has no room for an old Kitty image's complete clear escape
- **THEN** Grow leaves the clear out of the frame and retains its image ID for a later clear attempt

#### Scenario: Recursive or dashboard clear aggregation exceeds the output budget
- **WHEN** own, child, or another dashboard agent's Kitty clear does not fit the shared clear budget
- **THEN** Grow retains that image ID for a later clear attempt and emits no bytes beyond the budget

#### Scenario: Terminal decodes an accepted image
- **WHEN** a terminal receives an image escape sequence returned by Grow
- **THEN** Grow's output limit makes no claim about memory allocated by the terminal to decode or cache that image
