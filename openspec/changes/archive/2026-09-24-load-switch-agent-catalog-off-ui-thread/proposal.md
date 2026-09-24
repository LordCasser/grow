## Why

Creating a PromptWidget and opening the `/agent` picker both synchronously discover project, user, and plugin agent definitions. This can block first interaction or a key event on slow filesystems. The configuration modal now has an independent bounded worker, but the switch catalog remains on the UI path.

## What changes

- Seed prompt suggestions from static built-in Agent definitions without filesystem work.
- Open `/agent` with those immediately usable choices, then refresh the picker and suggestion catalog from a bounded background scan.
- Ignore late results for a superseded picker or changed session binding; preserve Workflow Run snapshot choices without scanning.

## Impact

Pager Agent switch suggestions and picker only. Shell remains the authority that accepts or rejects the selected Agent.
