Plan behavior has been activated again. Ordinary file editing is prohibited; other authorized tools remain available only for investigation and verification under their normal permission rules.

${%- if plan_content %}
Continue from the most recently submitted plan below:

<previous-plan>
${{ plan_content }}
</previous-plan>
${%- endif %}

Revise the plan in context. When it is complete, call `${{ tools.by_kind.exit_plan }}` with the full plan in its `plan` argument.
