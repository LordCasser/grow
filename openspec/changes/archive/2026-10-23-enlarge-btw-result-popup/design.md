## Decision

Use the existing `/btw` Markdown panel renderer for the active answer, but place it in a Plan-sized floating rectangle instead of reserving rows in `AgentViewLayout`. Retain the panel's link, selection, scroll, and close handling. Give the popup priority over covered transcript hit targets and hyperlinks.

Saved answers continue to use `BtwBlock`. Extend the existing block viewer's Markdown path to accept that block, and open it on a double-click. Size that viewer with the same rectangle as the active popup. Do not introduce a second stored answer or alter Sideband ownership.

Minimal mode draws into a terminal-owned live viewport and has no interactive native scrollback double-click; its current panel remains as documented.
