## ADDED Requirements

### Requirement: Direct Agent tool definitions preserve provider name constraints

The public Agent sampling-definition projection SHALL return only function names satisfying the existing cross-provider common format `^[a-zA-Z_][a-zA-Z0-9_-]{0,63}$`. It SHALL omit definitions outside that format from direct provider declarations while retaining their valid catalog identities in the underlying ToolBridge for meta-tool discovery and internal dispatch. It SHALL NOT truncate, rename or alias a tool to satisfy the provider limit, nor require Agent to depend on MCP's catalog validator.

#### Scenario: Provider length boundary

- **WHEN** a bridge contains valid qualified tools of 64, 65 and 256 characters
- **THEN** the public Agent sampling projection retains the 64-character definition and omits the longer definitions while the complete bridge catalog retains all accepted identities.

#### Scenario: Long names share a prefix

- **WHEN** distinct accepted catalog tools share their first 64 characters
- **THEN** direct projection does not manufacture a shared shortened name and meta-tool calls continue to address each full identity.

#### Scenario: Ordinary built-ins and short MCP tools

- **WHEN** names already satisfy the existing common provider format
- **THEN** their definitions, schemas and ordering are preserved by the projection.
