## ADDED Requirements

### Requirement: Ripgrep external programs require invocation authority

Grow SHALL conservatively classify an `rg` invocation containing an external-program argument shaped as `--pre` or `--hostname-bin`, in separate-value or equals form, as having executable effects beyond built-in observational search. Built-in safe-command and Auto routine shortcuts and a grant for a plain `rg` prefix SHALL NOT authorize that external program. Explicit exact-invocation grants and deny precedence SHALL retain their existing policy. Delegated capability projection SHALL require All rather than ReadExecute for such an invocation. Classification SHALL NOT stop at a `--` argument whose role as an option terminator has not been proved; conservative review of a literal flag-shaped pattern is permitted.

#### Scenario: Hostname helper without hyperlink output

- **WHEN** an `rg` invocation selects `--hostname-bin` while hyperlink or color output is disabled
- **THEN** the external-execution classification still applies before permission or delegated capability admission.

#### Scenario: Prefix and exact invocation grants

- **WHEN** the session has granted a plain `rg` prefix but receives an invocation with either external-program flag
- **THEN** it requires the ordinary additional authorization path; an existing exact grant for that invocation continues to obey the configured policy and deny rules.

#### Scenario: A separator-shaped option value does not hide execution

- **WHEN** `rg --glob -- --hostname-bin helper` uses `--` as the glob value before the execution option
- **THEN** permission and delegated capability classification still require the external-program authority.

#### Scenario: Ordinary search remains observational

- **WHEN** an ordinary `rg` search or `--pre-glob` filter contains no external-program flag-shaped argument
- **THEN** its existing observational classification and permission path remain available.
