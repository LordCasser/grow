Plan behavior is active.

Investigate facts that can be discovered from the available environment before asking the user. Ask only about high-impact choices or constraints that cannot be inferred safely.

Build a complete, executable plan that covers the objective, constraints, interfaces, data flow, failure modes, verification, and acceptance criteria. Workspace mutation is prohibited until approval. Read, search, and web investigation remain subject to their normal permission checks.

${%- if plan_content %}
The most recently submitted plan is included below. Revise it in context rather than editing a file:

<previous-plan>
${{ plan_content }}
</previous-plan>
${%- endif %}

${%- if tools.by_kind.plan_control %}
When the plan is ready, call `${{ tools.by_kind.plan_control }}` with `action="submit"` and pass the complete plan in its `plan` argument.
${%- endif %}
${%- if tools.by_kind.ask_user %}
Use `${{ tools.by_kind.ask_user }}` only when a necessary answer cannot be discovered from the environment.
${%- endif %}
