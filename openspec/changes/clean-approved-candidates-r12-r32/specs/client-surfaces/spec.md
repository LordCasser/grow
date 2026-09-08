## ADDED Requirements

### Requirement: Restore feedback does not require an unused degree cache
Pager SHALL present code-restoration summaries and failures without storing restore_degree in AgentSession or forwarding it through internal completion actions. Shared RestoreDegree wire types, parsing and validation SHALL remain intact.

#### Scenario: Loaded or forked restoration result
- **WHEN** session loading or worktree forking completes with a code-restoration summary
- **THEN** Pager displays the existing success or failure feedback and performs the existing follow-up actions without caching restore degree.
