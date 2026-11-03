## ADDED Requirements

### Requirement: Restored project instructions use the effective durable branch
Restoring an existing task SHALL use the ordered typed ProjectInstructions in its validated current Surface for model context, runtime source identity and the owning permission classifier. Current disk rules SHALL NOT replace or supplement those instructions during restore. A child sharing a permission manager SHALL NOT overwrite its parent classifier.

#### Scenario: Project rules changed on disk
- **WHEN** a suspended task is restored after AGENTS.md changed
- **THEN** its original effective durable rules are retained for model/source/classifier without rewriting its checkpoint, and unrelated recovery and authorization checks remain enforced.

#### Scenario: No durable rules
- **WHEN** an existing task without effective typed ProjectInstructions is restored after new rules are added on disk
- **THEN** the restored task retains the absence of project rules; untyped user text is not treated as project instructions.

#### Scenario: Effective branch changes
- **WHEN** compaction, rewind or fork changes the validated current Surface
- **THEN** rule selection follows the typed blocks retained on that branch in model order rather than physical log order.

#### Scenario: New child with a durable seed
- **WHEN** a new child or normalized fork is bootstrapped from its newly persisted seed
- **THEN** current project instructions are durably added once when absent; a verbatim mirror retains the inherited prefix exactly, and restoration of an existing child does not add current disk rules.
