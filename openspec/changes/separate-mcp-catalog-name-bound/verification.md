# Verification

## Source and scope

- Baseline: `8fbc52185ce30e38d97745dd6597242da1135c31` on `LordCasser/grow/main`.
- The recorded `rg --hostname-bin` and MCP cancellation deadline fixes are already present. Messages signature concatenation remains deliberately unadopted pending supported-route evidence, per the archived upstream review. This change implements only the accepted, unimplemented catalog/provider name separation.
- Agent keeps its existing dependencies and public return type. MCP keeps the full qualified identity and original visibility, disable and live-parent authorization owners. No aliases, truncated callable names, duplicated catalogs or new public validators are introduced.

## Completed checks

- Official Rust 1.93.1 toolchain; Linux x86_64. `cargo test --locked -p agent -p mcp --lib -- --test-threads=4`: Agent **504 passed**, MCP **200 passed**.
- Agent old-code control: removing only the projection filter makes `agent::tests::sampling_definitions_preserve_provider_names_and_complete_bridge_catalog` fail with eight declarations instead of the expected three; restoring the filter passes. The test drives the public Agent method and compares the unchanged complete ToolBridge catalog, schemas and relative order.
- MCP unit controls cover 64/65/256 acceptance, 257 rejection, both longest server and longest raw segments, existing syntax rejection, app-only metadata, descriptor path components and fallback namespace collision avoidance.
- Real filesystem probe: raw length 250 plus `.json` succeeds; raw length 253 plus `.json` fails with `ENAMETOOLONG`. The latter is a valid 256-character `s__<raw>` catalog identity. This justified the narrowly scoped descriptor layout adaptation.
- `openspec validate --all --strict --no-interactive`: **21 passed**, 0 failed before integration/archive.
- Integrated `cargo clippy --locked -p shell --lib --tests --no-deps` passes (exit 0); all Shell library and integration-test targets typecheck. Existing non-fatal warnings remain. This is compilation evidence, not runtime verification.
- Integrated `cargo fmt --all -- --check` and `git diff --check` pass.
- Independent read-only review found no remaining production or fixture defect after the exact actor permission test was strengthened to assert `policy_allow` and `policy_deny`, rather than accepting a generic reject caused by the default prompt policy.

## Pending checks

- Shell real registration → snapshot → search/schema → `use_tool` transport, app-only/disabled/live parent eligibility, exact actor permission rules, and descriptor repeat-write tests are implemented. Two local executable builds were externally terminated with exit 137 during Shell test code generation (first four jobs with the default linker, then one job with Rust's bundled lld). Neither reached test execution. Kernel evidence was unavailable, so these are recorded as build-environment failures rather than proven out-of-memory events. Further focused execution and CI remain required.
- Descriptor old-code control, final focused tests, strict/archive checks and final integrated CI remain pending. No remote push or CI pass is claimed here.

## Evidence limits

Local transport tests use the production ACP reverse bridge with an in-process MCP server; they do not claim validation against every remote provider or native Windows filesystem. The descriptor helper has no in-repository runtime caller at this baseline, but remains public and is covered through its actual async entry point. Existing best-effort handling for arbitrary upstream names outside the accepted catalog syntax/length, and pre-existing sanitizer collisions, are unchanged.
