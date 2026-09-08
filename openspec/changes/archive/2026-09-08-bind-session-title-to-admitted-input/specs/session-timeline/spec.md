## ADDED Requirements

### Requirement: Automatic title provenance identifies admitted user input
Automatic session-title Sidebands SHALL identify the durable user input whose text generated the title. Later notification or control events SHALL NOT replace that source identity merely by becoming the Timeline tail.

#### Scenario: Notifications arrive before title scheduling
- **WHEN** a user input commits and notification events append before title generation is scheduled
- **THEN** the title Sideband source identifies the admitted user input rather than the notification event

#### Scenario: Input identity cannot be proven
- **WHEN** title scheduling cannot establish the admitted user input identity
- **THEN** it does not start title generation with an unrelated Timeline tail reference

#### Scenario: Direct terminal command input
- **WHEN** a direct command input commits without a prompt index
- **THEN** its title scheduling uses the event returned by the durable user-message commit, which is acknowledged only after persistence succeeds
