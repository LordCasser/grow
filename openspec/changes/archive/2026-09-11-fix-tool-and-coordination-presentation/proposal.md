## Why

Two client-visible paths currently discard facts that the runtime already knows. An unknown tool name is reported as an argument parse failure, and Pager renders the same generic failure text as both error and output. Parent-child inquiries also reuse the cross-session `Answered session <id>` title even though the subagent coordinator knows the child task description. The first pair makes one failure look like two parse errors; the second hides which concurrent subagent participated.

## What Changes

- Preserve `ToolErrorKind::NotFound` through tool preflight as the existing `NonExistingTool` outcome, with an accurate model/user-facing message and no dispatch.
- Project failed generic tool content into one error surface instead of duplicating it as output.
- Carry the participating subagent task name from the coordinator into the receiving inquiry audit and use it for parent-child inquiry titles. Keep cross-session peer titles based on the source session identity.
- Keep coordination identity, replay coalescing, permission, tool execution, and sampling recovery behavior unchanged.

## Capabilities

- `session-timeline`: distinguish unknown tools from invalid arguments while retaining one durable failed tool result.
- `client-surfaces`: render a generic failed tool result once.
- `local-coordination`: identify parent-child inquiry rows by the participating subagent task name while retaining session identity for peer inquiries.
