# Design

Add an optional ordered list of Responses message boundaries to neutral `AssistantItem`. Each boundary indexes one UTF-8 slice of the existing flattened `content` and records only the provider-neutral phase (`commentary` or `final_answer`). The list includes empty output messages and contains neither native message ID nor status. A checked accessor verifies bounds and UTF-8 boundaries before request projection; malformed/stale ranges fall back to the existing single phase-less message. This avoids retaining a second copy of potentially large model text.

The terminal Responses conversion builds a flattened display string from each output message's text parts, separated by newlines, and records ranges. It retains all local function calls in the final Assistant item so the existing immediately following ToolResult batch remains valid. Existing native fragments continue to own same-route output order and opaque continuation.

Portable projection copies valid boundaries on unchanged assistant text. Redaction, truncation and compaction summary creation clear boundaries when they rewrite content. Fork CWD remapping rebuilds ranges from each segment while applying the same substitution, retaining phase semantics. A compacted span is an intentional summary, so only the un-compacted suffix keeps exact message phases.
