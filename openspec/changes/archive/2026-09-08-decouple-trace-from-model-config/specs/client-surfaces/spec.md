## ADDED Requirements

### Requirement: Trace export does not require valid model configuration
CLI trace dispatch SHALL proceed to session trace validation without requiring successful model configuration loading or AgentConfig construction. Existing session and output errors SHALL remain observable.

#### Scenario: Invalid model config and missing trace target
- **WHEN** model configuration TOML is malformed and trace requests a missing session
- **THEN** the command reports the missing session rather than aborting on model configuration parsing.
