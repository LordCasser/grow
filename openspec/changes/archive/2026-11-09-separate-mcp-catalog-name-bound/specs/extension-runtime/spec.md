## ADDED Requirements

### Requirement: MCP catalog identities have their own length bound

Grow SHALL retain unambiguous qualified MCP tool identities up to 256 ASCII characters in its existing catalog, subject to the existing first-character, character-set, server/tool segmentation, model-visibility and authorization rules. The qualified catalog bound SHALL be distinct from direct provider function-name limits. Accepted names SHALL remain unchanged across discovery, schema retrieval and internal `use_tool` dispatch; names longer than the bound SHALL be rejected without truncation, aliases or a second catalog.

#### Scenario: A qualified name exceeds provider length

- **WHEN** an otherwise eligible and syntactically valid MCP registration has a qualified name of 65 through 256 characters
- **THEN** the existing catalog and search/schema path retain that exact identity and internal dispatch invokes the original raw server tool name.

#### Scenario: Name exceeds the catalog bound or is ambiguous

- **WHEN** the qualified name has 257 characters or violates the existing segmentation or character rules
- **THEN** registration is rejected and no shortened or alternate callable name is manufactured.

#### Scenario: An app-only declaration has a long name

- **WHEN** a valid declaration within the catalog bound is restricted to app visibility
- **THEN** length acceptance does not make it model-callable or grant additional tool authority.


#### Scenario: The longest raw tool name needs a descriptor file

- **WHEN** an accepted qualified identity contains a raw name whose sanitized form plus `.json` exceeds a 255-byte filesystem component
- **THEN** descriptor materialization writes `tools/long-names/<complete sanitized name>/descriptor.json`, preserves the original name and schema in the JSON, and keeps ordinary flat descriptor paths unchanged.
