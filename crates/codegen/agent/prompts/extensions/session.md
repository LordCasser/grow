${%- if memory_enabled and tools.by_kind.memory_search and tools.by_kind.memory_get %}

<memory>
The session provides `${{ tools.by_kind.memory_search }}` and `${{ tools.by_kind.memory_get }}` for recalling prior decisions and context.
</memory>
${%- endif %}
