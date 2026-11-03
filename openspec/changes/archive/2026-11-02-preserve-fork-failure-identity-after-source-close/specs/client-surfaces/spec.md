## ADDED Requirements

### Requirement: Fork startup failure retains request identity after its source view closes

Retiring a source view SHALL NOT reclassify an in-flight fork as ordinary session creation. Initial startup failure SHALL retain the fork request's unsubmitted input using its existing pending fork identity even when its local source reference no longer exists. A bound replacement SHALL remain protected from initial failure.

#### Scenario: Source closes before worktree fork failure
- **WHEN** the source page closes before its unbound worktree fork reports startup failure
- **THEN** the directive is retained in the failed draft without automatic execution or source resurrection.

#### Scenario: Copy succeeds but initial replay fails
- **WHEN** a copied fork fails its initial history load before submitting its directive
- **THEN** its unsent content remains in a failed draft and cleanup does not delete the durable copy.
