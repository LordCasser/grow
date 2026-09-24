## ADDED Requirements

### Requirement: Subagent permission audit details are bounded and redacted

Live and durable subagent permission audit projections SHALL use the same bounded, redacted access summary and harness-owned decision reason. They SHALL NOT expose raw access detail or free-form classifier prose. The visible summary SHALL retain the tool identity and access kind where available; request arguments, paths, commands, URL credentials, path, query and fragment SHALL remain redacted. The projected access summary SHALL be at most 240 bytes, including its tool identity.

#### Scenario: Live permission decision contains sensitive request data
- **WHEN** a subagent permission decision contains a command, path, MCP arguments, URL credentials, or classifier prose with sensitive content
- **THEN** the live Pager detail displays only bounded redacted audit fields and contains none of the raw request or classifier prose.

#### Scenario: Permission decision is replayed
- **WHEN** Pager reconstructs the same decision from durable session updates
- **THEN** it displays the same bounded redacted audit projection as the live notification.
