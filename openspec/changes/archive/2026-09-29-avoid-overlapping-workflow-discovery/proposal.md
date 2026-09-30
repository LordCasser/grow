# Why

The Workflow Registry independently scans the project and user workflow directories. When the session project root is the user's home directory, both computed paths are `~/.grow/workflows`. A single file is read twice and listed as two Definitions with different scopes, even though the bundled `deep-research` file is a User workflow.

# What Changes

- If the project and user workflow directory paths coincide or open as the same directory entity, scan it once as User.
- Keep separate Project and User Definitions when their directories differ, including Definitions with the same name.
- Add a Registry regression test for the overlapping-root case and document the discovery rule.

No Workflow Run, persistence, or wire format changes are required.
