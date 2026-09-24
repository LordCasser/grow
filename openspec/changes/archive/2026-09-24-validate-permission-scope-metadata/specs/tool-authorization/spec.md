## ADDED Requirements

### Requirement: Remembered permission scopes are bound to the current access

A remembered MCP or Bash permission scope SHALL be derived from, or validated against, the actual `AccessKind` for the permission request. MCP tool scope SHALL name that exact tool; MCP server scope SHALL name the server parsed from that tool's qualified identity. Bash selected command terms SHALL be a non-empty prefix of the request's primary command terms. Invalid, mismatched, or access-inappropriate selection metadata SHALL NOT create a remembered scope from the supplied metadata and SHALL fall back to the request-derived scope where one exists.

#### Scenario: MCP response names another tool
- **WHEN** an allow-always MCP response contains a well-formed tool scope naming a different tool
- **THEN** the remembered outcome is limited to the current request's MCP tool.

#### Scenario: MCP response names another server
- **WHEN** an allow-always MCP response contains a well-formed server scope that differs from the server parsed from the current tool identity
- **THEN** the remembered outcome is limited to the current request's MCP tool.

#### Scenario: Bash response selects unrelated command terms
- **WHEN** an always-allow or always-reject Bash response contains well-formed command terms that are not a non-empty prefix of the current primary command terms
- **THEN** the outcome uses the scope derived from the current command script.

#### Scenario: Request option metadata disagrees with MCP tool identity
- **WHEN** Pager receives a permission request whose MCP scope option metadata names another tool or an inconsistent server prefix
- **THEN** Pager does not expose a server scope toggle from that metadata.
