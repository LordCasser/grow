<runtime_context>
OS: ${{ os_name }}
Shell: ${{ shell_path }}
Workspace: ${{ working_directory }}
Current date: ${{ current_date }}
</runtime_context>
${%- if memory_enabled and tools.by_kind.memory_search and tools.by_kind.memory_get %}

<memory>
The session provides `${{ tools.by_kind.memory_search }}` and `${{ tools.by_kind.memory_get }}` for recalling prior decisions and context.
</memory>
${%- endif %}
${%- if role_instructions %}

<role-instructions>
${{ role_instructions }}
</role-instructions>
${%- endif %}
${%- if persona_instructions %}

<persona>
${{ persona_instructions }}
</persona>
${%- endif %}
