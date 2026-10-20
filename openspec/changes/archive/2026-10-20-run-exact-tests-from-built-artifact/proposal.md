# Run exact Cargo tests from the compiled artifact

## Why

The exact-test runner invokes Cargo separately for compilation, test enumeration and execution. The later invocations can repeat build-script work and rebuild an already compiled test target, making the short enumeration deadline fail on native CI before the selected test is checked.

## What Changes

- Compile once with Cargo JSON messages and select the unique test executable from `compiler-artifact` records with `profile.test == true` and a nonempty executable.
- Enumerate and execute the exact test by invoking that same executable directly, retaining exact-name, ignored-test and one-successful-test proofs.
- Preserve compiler diagnostics and Cargo progress while suppressing bulky artifact JSON; fail closed if the artifact is missing or ambiguous.
- Add a real Rust fixture whose build script reruns on every Cargo invocation, proving one runner invocation performs one Cargo build only.

## Impact

- Affected spec: `development-workflow`
- Affected code: exact Cargo runner and its Python tests.
