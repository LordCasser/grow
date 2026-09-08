## ADDED Requirements

### Requirement: Sips output reads have an encoded budget
Sips output loading SHALL accept only non-empty regular-file data of at most100,000,000 bytes and consume at most that allowance plus1 byte. Unix output opening SHALL reject symlinks and avoid blocking on FIFOs.

#### Scenario: Empty oversized or unreadable output
- **WHEN** output is empty, too large or unreadable
- **THEN** reject the sips result, release its owned temporary directory and retain existing conversion fallback behavior.

#### Scenario: Exact budget and growth after metadata
- **WHEN** actual bytes exactly fit the allowance or exceed it after metadata checking
- **THEN** preserve exact-fit data, or reject after consuming at most allowance plus1, respectively.
