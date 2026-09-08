## ADDED Requirements

### Requirement: Deferred file URLs survive classification miss
Agent and Dashboard clipboard completion SHALL preserve non-empty unclassified file URLs as text after a successful no-raster probe when the source has no non-whitespace original text.

#### Scenario: No original text and no classified attachments
- **WHEN** a successful file URL probe yields no classified entries and original text is missing, whitespace-only or unreadable
- **THEN** insert the raw URL text once and report its insertion result instead of treating the clipboard as empty.

#### Scenario: Original text or classified entries exist
- **WHEN** original non-whitespace text exists or file classification already produced a handled/rejected result
- **THEN** keep existing insertion behavior without additionally inserting raw URL fallback text.

#### Scenario: Probe or target is not admissible
- **WHEN** the attachment probe failed, was dropped, persistence failed, or existing question/stale target guards reject the completion
- **THEN** do not insert URL fallback text.
