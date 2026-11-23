## ADDED Requirements

### Requirement: Foreground compaction waits for an invalidated source to retire
Foreground automatic compaction SHALL await the existing background worker's settlement and durable terminal before starting a replacement transaction when the frozen source is no longer current.

#### Scenario: Surface rewrite invalidates a running summary
- **WHEN** a foreground turn needs compaction after a rewrite invalidated the unfinished background summary's selected Surface IDs
- **THEN** the worker can settle its accounting, its late output is discarded, and its cancellation terminal precedes the new start. A failed durable terminal prevents the new start.

### Requirement: Settling pending skill loads preserves surviving consumption evidence
Skill settlement SHALL preserve original load coordinates for surviving SkillLoad nodes in branch replay and checkpoint projections.

#### Scenario: Consumed and fresh skills share a rewritten Surface
- **WHEN** an admitted response consumed historical skills and a later terminal settles newer unconsumed skills
- **THEN** surviving skills retain their consumption status across replay and checkpoint recovery, remain eligible for ordinary compaction, and a repeated settlement leaves their tool results intact.

#### Scenario: A previous writer already removed the relabeled cohort
- **WHEN** cold replay encounters an already committed settlement that removed every surviving SkillLoad after an earlier settlement lost their load coordinates, with no intervening admitted response
- **THEN** replay verifies the earlier settlement and the exact transformation of the cohort and its paired results. Live writes still settle only currently unconsumed skills.

### Requirement: A missing preventive compaction range yields to request admission
If the planner finds no eligible range, preventive automatic compaction SHALL let an otherwise fitting request proceed through the ordinary admission boundary and suppress repeated attempts within the turn.

#### Scenario: Threshold crossed while the request fits
- **WHEN** estimated input crosses the configured threshold and the current request including its explicit output reserve fits the model window, but retained history has no eligible closed range
- **THEN** the turn can send its primary request without emitting a successful summary or repeating the same compaction attempt. Live, replay and headless presentation distinguish this skip from user cancellation.

#### Scenario: Compaction is required or an actual error occurs
- **WHEN** recovery follows a provider overflow, the request exceeds its hard window, the user explicitly requested compaction, or a durable write fails
- **THEN** lack of a usable summary does not authorize dispatch or acknowledge the manual operation as successful. Storage failures propagate to their owner.
