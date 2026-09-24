## ADDED Requirements

### Requirement: Non-macOS clipboard image work has an in-process allowance
Non-macOS arboard image reads SHALL hold one process-wide permit through read and PNG encode, including after a caller timeout. Grow SHALL reject returned RGBA images over 16,000,000 pixels before PNG encoding, and SHALL refuse PNG output over 50,000,000 bytes. The limits SHALL NOT be described as a bound on platform RGBA allocation before `get_image` returns.

#### Scenario: Timed-out image reader remains active
- **WHEN** an arboard image worker has not returned by the caller deadline and a second image read starts
- **THEN** the second read does not start another in-process image worker while the first remains active.

#### Scenario: Oversized RGBA image or PNG output
- **WHEN** returned dimensions exceed the pixel allowance or PNG encoding would exceed the output allowance
- **THEN** Grow returns an error without retaining an over-limit encoded image.

### Requirement: Linux clipboard image helper capture is bounded
Linux CLI image fallback SHALL retain at most 50,000,001 stdout bytes, and SHALL reject an image whose output exceeds 50,000,000 bytes. A failed or over-limit helper SHALL not be reported as an empty clipboard image.

#### Scenario: Helper writes beyond the image allowance
- **WHEN** a Linux clipboard helper writes more than 50,000,000 image bytes
- **THEN** Grow reports an over-limit error and reaps or kills the helper within the existing deadline.
