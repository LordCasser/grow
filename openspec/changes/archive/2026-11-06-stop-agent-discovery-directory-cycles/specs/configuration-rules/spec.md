## ADDED Requirements

### Requirement: Agent discovery terminates physical directory cycles

Recursive file-defined Agent discovery SHALL compare each directory against the physical directory identities on its current ancestor chain before descending. A repeated ancestor or unresolvable directory identity SHALL be skipped. Traversal identity SHALL NOT replace lexical path-derived Agent namespaces or globally suppress a legal alias reached through another branch. Existing stable path order, source scope and precedence SHALL remain unchanged.

#### Scenario: Self or parent link forms a cycle

- **WHEN** an Agent directory contains a link to itself or a physical ancestor
- **THEN** discovery terminates at that link without repeatedly advertising the same physical definitions under deeper aliases, while ordinary definitions remain available.

#### Scenario: Independent aliases are valid namespaces

- **WHEN** separate sibling directory aliases reach the same valid non-ancestor target containing Agent definitions
- **THEN** each lexical namespace remains discoverable in stable order and neither is suppressed by the other branch's traversal history.
