## ADDED Requirements

### Requirement: Background image viewer source bytes are bounded
The background image viewer loader SHALL admit only non-empty sources up to50,000,000 encoded bytes, checking memory length before copying and consuming at most allowance plus1 byte from files.

#### Scenario: Source exceeds allowance
- **WHEN** memory length, file metadata or actual file bytes exceed the allowance
- **THEN** return a failed load before decoding/conversion, preserving existing viewer failure completion handling.

#### Scenario: Valid file or memory source
- **WHEN** source bytes exactly fit the allowance and contain a valid image
- **THEN** retain source data and existing protocol conversion behavior for both representations, including valid symlink file sources.

#### Scenario: Special file or growing source
- **WHEN** an opened source is not a regular file or its content grows beyond the allowance
- **THEN** reject it; Unix FIFO opening must not block, and actual reading stops at allowance plus1.
