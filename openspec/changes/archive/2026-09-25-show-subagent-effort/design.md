# Design

`SubagentSpawnEvent.reasoning_effort` is the canonical effective value. Add an optional effort string to `SubagentSpawned` and populate it from this value in both the live producer and the spawn-fact replay projection. The child model catalog's static default is not sufficient: a child may have an explicit override.

Pager stores the projected effort beside `SubagentInfo.model`. On spawn it also seeds the child `ModelState` with that value. On child `ModelChanged`, the existing `sync_child_control_projection` copies both the current model and effort to the parent index. Replay merge must not replace a newer live effort with an older spawn value. If child updates are restored during spawn or when the detail is opened, the parent index is synchronized after that replay when the child has a selected model.

The two requested surfaces share a compact model/effort formatter. Existing width and right alignment calculations include the suffix; on narrow terminals the description yields space first. No separate column, badge or color is added.
