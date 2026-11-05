## ADDED Requirements

### Requirement: Responses function-call starts may omit the empty argument prefix

Responses decoding SHALL accept an absent arguments field only in a response.output_item.added function_call item explicitly marked in_progress, treating that absence as an empty streaming prefix. The original provider evidence SHALL remain unchanged. This exception SHALL NOT supply arguments to a closed or terminal item or bypass identity, completion, parameter JSON or recovery-budget validation.

#### Scenario: Initial function item omits arguments

- **WHEN** an in_progress function_call added event omits arguments and later matching argument events and a valid terminal response supply the complete call
- **THEN** sampling receives the stream and completes with the supplied arguments rather than failing at the initial item.

#### Scenario: The initial item already carries a prefix

- **WHEN** a function_call added item carries a valid string arguments prefix
- **THEN** the prefix remains unchanged and subsequent arguments and terminal snapshots must satisfy the existing consistency rules.

#### Scenario: Arguments are malformed or required on a completed item

- **WHEN** arguments is null or a non-string, or a done/terminal function_call omits arguments
- **THEN** the known event remains a decoding failure and is not repaired by the initial-prefix exception; SDK rejection of duplicate known fields also remains effective before decode-copy normalization.

#### Scenario: No valid completion follows the initial item

- **WHEN** the initial function item is accepted but the stream lacks valid completion or subsequent identities or arguments conflict
- **THEN** sampling rejects the candidate under the existing protocol and incomplete-stream rules and does not admit an executable call.
