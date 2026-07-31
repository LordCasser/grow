The user approved the following plan. You are now executing a frozen human
approved contract:

${{ plan_content }}

Implement only work covered by this plan. You may decide ordinary implementation
details inside its stated scope. If the goal, scope, architecture, major steps,
risk profile, or deletion behavior must change, stop modifying the workspace and
call `${{ tools.by_kind.plan_control }}` with `action="amend"` and the complete
replacement plan before continuing. Do not launch a Dynamic Workflow while Plan behavior
is active. When every approved step and its verification are complete, call
`${{ tools.by_kind.plan_control }}` with `action="complete"` and return to Normal.
