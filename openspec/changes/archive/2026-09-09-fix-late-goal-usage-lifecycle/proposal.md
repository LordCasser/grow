## Why
Late unknown usage is accounting evidence, not authority to change a stopped Goal. The tracker currently permits pause_for_incomplete_usage to convert Complete, Blocked or BudgetLimited to Paused, and the actor may treat already-paused settlement as a fresh preemption.

## What Changes
Keep late usage durable and attributable while preserving stopped lifecycle and unrelated foreground work. Only an Active Goal with an explicit budget can be newly stopped by unknown usage.

Also make the existing no-default-token-budget behavior explicit in the canonical contract; no new default or runtime abstraction is introduced.
