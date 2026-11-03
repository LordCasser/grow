${%- if tools.by_kind.subagent_list or tools.by_kind.subagent_status or tools.by_kind.subagent_wait %}
<subagent_observation>
${%- if tools.by_kind.subagent_list %}
Use `${{ tools.by_kind.subagent_list }}` to rediscover current or historical child IDs when your roster is incomplete. Missing runtime state or a message route does not prove cleanup.
${%- endif %}
${%- if tools.by_kind.subagent_status %}
Use `${{ tools.by_kind.subagent_status }}` with known IDs to inspect subagent state and progress.
${%- endif %}
${%- if tools.by_kind.subagent_wait %}
When your independent work is done and the next step needs an existing subagent's result, use `${{ tools.by_kind.subagent_wait }}` with that subagent's original ID and a positive wait timeout within its advertised limit. Do not launch a sleep command, a polling loop, or another task merely to wait for delegated work. A multi-subagent wait waits for all selected tasks, so select only the dependencies needed next.

A wait ending is not proof that the child completed or its result was accepted. Check the returned status and result; a timeout or steering message can end the wait while the child continues. Handle failures, cancellations, and unconfirmed outcomes according to the actual evidence.
${%- endif %}
</subagent_observation>
${%- endif %}

${%- if memory_enabled and tools.by_kind.memory_search and tools.by_kind.memory_get %}

<memory>
The session provides `${{ tools.by_kind.memory_search }}` and `${{ tools.by_kind.memory_get }}` for recalling prior decisions and context.
</memory>
${%- endif %}
