# Why

Workflow manager fixtures freeze the real host's user Agents and Skills through `WorkflowAgentCatalogSource::for_test`. The recorded fixed-binary comparison found 25 failures without isolation and 147 passing tests with isolated HOME/GROW_HOME. A normal developer test command must not depend on private user resources or require manually sanitized environment variables.

# What Changes

Run Workflow tests that capture live discovery in an exact-name child using the existing `TestSandbox` and `TestProcess` owners. Keep the fixture's real discovery and all existing assertions. Add a polluted-user-resource regression proving the child excludes ambient sources while explicit project fixtures remain discoverable. Cover adjacent actor tests that launch the same fixture.

# Impact

Test-only tooling maintenance with `skip_specs: true`. No production changes, new discovery API, process-global environment mutation, or manifest-limit increase. The ordinary 512 KiB manifest rejection remains tested. Existing isolated cold-recovery tests outside this scope remain unchanged.
