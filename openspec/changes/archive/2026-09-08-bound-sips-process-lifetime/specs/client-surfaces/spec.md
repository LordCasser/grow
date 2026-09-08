## ADDED Requirements

### Requirement: Sips converter has an owned execution deadline
The spawned sips converter SHALL have a 10-second execution deadline and an independently owned process group. Ordinary completion, timeout and wait failure SHALL attempt group cleanup before returning to conversion-file cleanup.

#### Scenario: Converter stalls
- **WHEN** the spawned converter does not exit within its execution allowance
- **THEN** report timeout, kill the owned group and reap the direct child before releasing its conversion scope.

#### Scenario: Leader exits with descendants
- **WHEN** the converter leader exits but members of its owned group remain
- **THEN** terminate the remaining group before returning its exit status.

#### Scenario: Cleanup fails
- **WHEN** process cleanup fails for a reason other than an already-gone Unix group
- **THEN** report the cleanup failure rather than presenting conversion as successfully cleaned up.
