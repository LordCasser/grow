# Verification

## Contract and implementation

- Precise entry-top and entry-center navigation use the same effective sticky-layout gate as rendering. The effective header is empty when sticky headers are disabled or compact prompt mode is active; enabled sticky mode retains the renderer's sticky calculation.
- The regression covers sticky headers off, sticky headers on, and compact prompts. It renders through `ScrollbackPane` and checks the selected entry's actual rendered area for top and center placement. Its sticky-on target follows response content so the assertion exercises a stable header/content layout rather than the inline-prompt transition boundary.

## Test results

- Pager suite: **7,620 passed, 0 failed, 16 ignored** (`/tmp/grow-2.5.2-pager-final3.log`). `precise_entry_navigation_matches_sticky_display_setting` passed.
- Shell suite: **4,413 passed, 0 failed, 9 ignored** (`/tmp/grow-2.5.2-shell-last.log`).
- Settings suite: **235 passed, 0 failed, 0 ignored** (`/tmp/grow-2.5.2-settings.log`).
- Native MCP reload probe: **1 passed, 0 failed, 0 ignored** (`/tmp/grow-2.5.2-mcp-probe.log`).
- Strict OpenSpec validation: **33 passed, 0 failed** (`/tmp/grow-2.5.2-specs-prearchive.log`).

归档核对发现原 delta 误标 MODIFIED，主规范没有该命名条目；已改为 ADDED，明确是新增精确导航约束，未删除或替换既有行为条目。
