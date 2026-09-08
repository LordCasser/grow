## ADDED Requirements

### Requirement: Credential helper failures do not echo output payloads
Credential helper failures SHALL NOT include raw stdout or stderr content in returned or logged failure messages. Diagnostics SHALL preserve structural failure information without credential values. Pipe draining and output limits SHALL remain in force.

#### Scenario: Helper stderr contains a credential
- **WHEN** a helper exits unsuccessfully after writing credential-bearing stderr
- **THEN** the failure reports exit status and output size without echoing stderr.

#### Scenario: Malformed JSON contains sensitive values
- **WHEN** token JSON cannot be decoded because a field contains an invalid value
- **THEN** the failure reports JSON category and location without embedding that value.
