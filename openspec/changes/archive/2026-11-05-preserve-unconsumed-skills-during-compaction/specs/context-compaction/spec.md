## ADDED Requirements

### Requirement: Compaction selection preserves unconsumed skill causality

Compaction planning SHALL consume unconsumed SkillLoad identity from the same authoritative Timeline materialization as its source Surface and IDs. It SHALL exclude each unconsumed skill and its complete model load call/result response group from both summary input and replacement target, preserving original identity and body in the retained tail. Summary Sideband completion SHALL NOT count as skill consumption. Existing final Timeline protection, canonical response consumption and explicit terminal settlement semantics SHALL remain unchanged. Consumed skill history SHALL regain ordinary range eligibility. Initial and budget-reduced ranges SHALL share these constraints.

#### Scenario: Pending model skill overlaps an old candidate
- **WHEN** a model load_skill exchange has no later selected canonical response and falls inside the ordinary old-history candidate
- **THEN** compaction can summarize a sufficient earlier complete prefix while retaining the original skill body, identity, call and result for the next main request, without rerunning the tool.

#### Scenario: No adequate prefix precedes a protected skill
- **WHEN** excluding protected skill causality leaves no range meeting source minimum and retained-tail budget
- **THEN** no summary provider request starts and the current Surface remains unchanged; a summary is not generated only to fail final causal validation.

#### Scenario: A canonical response consumes the skill
- **WHEN** a real branch-selected canonical response is admitted after the skill load
- **THEN** that history can participate in ordinary later compaction; summary or raw assistant text alone cannot grant this eligibility.

#### Scenario: Frozen generation and input fitting
- **WHEN** a protected range is frozen for async compaction or reduced to meet source budget
- **THEN** all retries and final target stay within the same safe source identities, and late publication still uses the existing authority and Timeline protection checks.
