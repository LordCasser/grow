## ADDED Requirements

### Requirement: Image normalization uses the bounded compute path
Image normalization SHALL run through its existing cancellation-safe worker admission without an inactive process-wide normalization cache or its unused remote activation flag. Image format conversion, integrity checks, size and pixel limits, original-content fallback, attachment metadata and per-image feedback SHALL retain their existing behavior.

#### Scenario: Normalize repeated attachments
- **WHEN** attachments with identical content are admitted
- **THEN** each uses the supported normalization path and produces the same valid content and per-attachment metadata without consulting a disabled cache.

#### Scenario: Cancel a normalization waiter
- **WHEN** a caller is cancelled after blocking normalization has started
- **THEN** the running worker retains admission capacity until its actual work completes.

#### Scenario: Re-encoding cannot meet the bound
- **WHEN** normalization cannot produce an encoding under its byte limit
- **THEN** the original attachment and its indexed fallback notice remain available.
