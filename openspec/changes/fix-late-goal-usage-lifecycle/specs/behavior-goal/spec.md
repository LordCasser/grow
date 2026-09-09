## ADDED Requirements

### Requirement: Late usage preserves stopped Goal lifecycle
Usage admitted under a Goal SHALL remain attributable after it stops. Recording late unknown usage SHALL preserve the existing stopped status and SHALL NOT preempt unrelated foreground work. Only a currently Active Goal with an explicit token budget may be stopped by incomplete usage.

#### Scenario: Late unknown usage after completion or another stop
- **WHEN** an admitted attempt settles without usage after its Goal became Complete, Blocked, BudgetLimited or Paused
- **THEN** the incomplete-usage marker is persisted while the stopped status remains unchanged and unrelated foreground work is not preempted. An already-paused Goal may still stop a retry belonging to its own retiring turn.

#### Scenario: Unknown usage during an active unbudgeted Goal
- **WHEN** an Active Goal has no explicit token budget and usage is unknown
- **THEN** usage remains a lower bound, admission remains open and the Goal continues.

#### Scenario: Unknown usage during an active budgeted Goal
- **WHEN** an Active Goal has an explicit token budget and usage is unknown
- **THEN** provider admission closes and the Goal pauses at its safe step boundary.

### Requirement: Goal token budgets are explicitly opt-in
Goal SHALL have no token spending limit when creation omits token_budget. Persistence, restore, continuation and usage settlement SHALL preserve the absence of a budget without substituting a default cap. Editing an existing explicitly budgeted Goal without a budget argument SHALL preserve that explicit budget until the user removes it.

#### Scenario: Create and restore without a budget
- **WHEN** a Goal is created without token_budget and later restored
- **THEN** its token_budget remains absent and cumulative token usage does not impose a spending limit.

#### Scenario: Incomplete usage without a budget
- **WHEN** an unbudgeted Goal receives missing provider usage
- **THEN** it retains a lower-bound usage ledger and can continue without an inferred token limit.

#### Scenario: Preserve an explicit budget during objective editing
- **WHEN** an explicitly budgeted Goal is edited without changing the budget
- **THEN** the existing explicit budget remains until an explicit budget-removal operation.
