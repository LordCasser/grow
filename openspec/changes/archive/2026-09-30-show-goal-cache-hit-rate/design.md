## Existing surfaces

`GoalTokenUsage` retains cached, uncached, and unclassified input separately. `goal_detail::usage_lines` renders the counts; `agent_status::goal_status_line` occupies the ordinary usage slot while a Goal exists; `transcript_projection` reproduces Goal usage in read-only details. The ordinary status and `/usage` use `status_blocks::cache_hit_rate` to divide known cached input by the input whose cache-read count is known.

## Projection

The Goal measured denominator is cached plus uncached input. Reuse `cache_hit_rate` on that denominator, returning N/A if it is zero or cannot be represented. Mark a non-N/A ratio as measured when the Goal has unclassified input, historical aggregate consumption without categories, or incomplete usage. Do not infer that missing cache data means a zero cache hit. Compute this in one Pager helper so the three Goal surfaces cannot drift; no Shell state or wire schema changes are needed.

The compact chip keeps its existing token/budget and elapsed fields. The detail overlay adds one line next to the cache counts. The read-only Goal notice adds the same percentage to its details; source Timeline records remain unchanged.

## Verification

Exercise exact, partially classified, historical, incomplete, zero-input, and overflow cases in Pager tests. Verify that non-Goal Plan/Workflow still use the ordinary usage status. Run focused Pager tests, formatting, and strict OpenSpec validation.
