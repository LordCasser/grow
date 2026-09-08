## ADDED Requirements

### Requirement: Fallback session titles fit canonical title bounds
Fallback session titles SHALL be non-empty and at most 160 Unicode scalar values after whitespace normalization. They SHALL retain the existing first-ten-word selection and use the default title for empty source text.

#### Scenario: Long unbroken user text
- **WHEN** title generation fails and the user text contains a long URL, CJK paragraph or other long word
- **THEN** the fallback is truncated safely to fit the canonical title character limit

#### Scenario: Ordinary or empty source text
- **WHEN** fallback source text is short or empty
- **THEN** existing first-ten-word or default-title behavior is retained
