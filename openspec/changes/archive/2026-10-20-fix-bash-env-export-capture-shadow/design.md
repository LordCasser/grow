## Context

`dump_bash_state` calls `export -p` after declaring local `__grow_dump_allexport` and local `env_vars`. Bash's dynamic scoping causes either local to hide a same-named global during the builtin capture. Renaming a local merely selects another possible user variable name and does not close the collision class.

## Decision

Avoid every local declaration before export capture. Save `$-` in `dump_bash_state`'s function-local positional parameters, then disable allexport based on that saved flag. Function positional parameters are scoped to the call and do not introduce a variable name that can collide with user exports. Pass the filtered `export -p` command substitution directly as `_emit_encoded`'s first argument, so capture completes before encoder locals exist. Restore the original allexport state from the saved positional value after the other snapshot blocks. The encoded block format and external filtering pipeline remain unchanged. Existing explicit `local +x` declarations for later payloads remain because they are introduced after export capture and prevent allexport from putting payloads into subprocess environments.

## Checks

Exercise exported `env_vars` and allexport-bookkeeping-name values through two production prepare_command/fd dump/replay cycles, with allexport both enabled and disabled. Confirm their values and export attributes survive, the original allexport state remains intact, and a large unrelated function still serializes. Also run a controlled Bash source-script probe to isolate the shell scoping rule from Rust transport.
