# Design

The workspace permission mapper is the authority boundary because it has the frozen `AccessKind`. An MCP tool selection is accepted only for the exact current tool. A server selection is accepted only for the server parsed from that exact qualified tool name. Invalid or mismatched metadata falls back to the current tool's scope. Bash selected terms are accepted only when they form a non-empty prefix of the current script's primary command words; otherwise the existing script-derived primary-command fallback applies. Specialized option IDs on another access kind cannot turn response metadata into a remembered scope.

Pager validates MCP option metadata against the tool title carried by the same permission request and verifies the server prefix derived from that title before making the scope toggle available. The workspace-side validation remains authoritative if a client sends forged response metadata.
