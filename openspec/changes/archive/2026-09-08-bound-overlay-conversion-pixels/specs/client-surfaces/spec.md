## ADDED Requirements

### Requirement: Kitty image conversion bounds source pixels
Grow SHALL require nonzero dimensions and at most 16,000,000 source pixels before converting non-PNG image bytes for Kitty overlays, including the macOS sips backend.

#### Scenario: Conversion source exceeds pixel budget
- **WHEN** a non-PNG source header exceeds the pixel allowance or has invalid dimensions
- **THEN** return no converted preview bytes before invoking a conversion backend, preserving the original attachment bytes.

#### Scenario: Valid conversion and direct transmission
- **WHEN** source dimensions fit the conversion budget or the protocol directly transmits encoded data
- **THEN** retain existing backend selection and direct-transmission behavior; this limit does not claim to bound terminal-side decode memory.
