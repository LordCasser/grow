## Context

`FinalizedToolset::try_parse` already returns a typed `NotFound` error, but session preflight currently routes every error through `handle_tool_parse_error`. Pager's generic tool projection then stores the same ACP content in both `OtherToolCallBlock.error` and `.output`, and expanded rendering shows both fields.

For coordination, `SubagentCoordinator` resolves the exact active child and owns its `SubagentRequest.description`, but `InboundInquiry` and `IncomingInquiryAudit` retain only session ids. Consequently `IncomingInquiryAudit::notice` cannot distinguish a local delegation from a peer-session inquiry without guessing from UI state.

## Decisions

### Branch on the existing tool error discriminator

Session preflight will inspect `ToolError.kind`. `NotFound` returns the existing `ToolLoop::NonExistingTool`, records a `tool_not_found` diagnostic, emits one failed ACP update and appends one tool result explaining that the name was unavailable and was not executed. Other failures keep the current parse-error path. No aliases, fuzzy matching, new loop outcome, or sampling retry are introduced.

### A failed generic content block has one presentation role

Pager will store non-empty generic ACP content as output only for successful calls. Failed calls store the same content only as error, with `Failed` as the empty-content fallback. This preserves all text while preventing duplicate expanded rendering.

### Persist delegation presentation identity at the authority boundary

When `SubagentCoordinator` resolves a direct parent-child route, it will pass the matched child's task description to the Shell runner. An empty description falls back to the subagent id so concurrent children remain distinguishable. `InboundInquiry` and `IncomingInquiryAudit` carry this as an optional `delegated_subagent_task_name`; peer-runtime inquiries leave it absent.

`IncomingInquiryAudit::notice` will select one participant label:

- delegation: `subagent <task name>`;
- peer coordination: `session <source session id>`.

Status wording is composed around that participant, producing titles such as `Answering subagent TS registry workload presentation`, `Answered subagent TS registry workload presentation`, and the existing `Answered session <id>`. The structured field is persisted with the inquiry audit so live updates, reconnect, and replay use the same identity. Missing historical fields naturally retain the peer-session fallback.

## Non-goals

- Renaming registered tools or accepting invented tool aliases.
- Automatically resampling complete unknown-name tool calls.
- Introducing a dedicated coordination row type or changing row color, layout, folding, timing, or coalescing.
- Replacing session ids with discovered session titles for cross-session peers.

## Verification

- Shell/tool tests distinguish unknown names from malformed/invalid arguments and assert zero dispatch with exactly one failed result.
- Pager tests assert a failed generic result has error text but no duplicate output, while successful generic output remains intact.
- Coordinator tests assert both parent-to-child and child-to-parent routes carry the matched child task name.
- Coordination audit and Pager tests assert delegation titles use the task name, peer titles keep session identity, and replay updates the same row.
- Run the affected crate tests, strict OpenSpec validation, archive the change, then validate current and archived specs again.
