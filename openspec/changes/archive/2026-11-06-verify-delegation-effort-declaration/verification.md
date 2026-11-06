# Verification

## Original schema and process version

Parent read the investigation and independently reran the bounded projection of original Responses requests acd35e44-ca54-487b-bcb3-e888bde6352e (seq17664) and938664d7-06e9-4e2e-a101-a04cf28a84ef (seq17679), exit0. Actual spawn_subagent schema SHA256 is the same d208ff7a26f0f24391bfb0bd83512dfedc2e4028663a0916787577b01be108ae in both; properties do not contain effort, and required contains onlyprompt/description. Unified logs in the corresponding timestamp window identify pid46777/version2.6.0. The exact old PID source commit was not recorded, so the currently installed command's separate commit is not attributed to that PID.

Fixed public2.6.1annotated tag peels todeaca4899bdec260ddc92a7bc98ceaa8da0ee5bd. Its TaskToolInput includes optional effort, required parent_step_id and per-field resume overrides. Parent checked the registry/schema/builder path and the documented user-guide fields. Evidence is `/tmp/grow-effort-investigation-01a1a9c0/{findings.md,project_schema_evidence.py,schema_projection.json}` and independently generated `/tmp/grow-effort-parent-proof.json`. No original user/process/config/provider was changed.

## Published executable's actual declaration

A fresh bounded verifier executed `/usr/bin/python3 /tmp/grow-effort-release-probe/probe.py`, launching only `/tmp/grow-261-published-42138009783/grow --cwd <isolated-project> -p 'Say Done.' --model mock/parent --max-turns 1 --no-memory --permission-mode always-approve --output-format json` with a temporary GROW_HOME. All configured provider/service endpoints point to its loopback mock. HOME was not overridden; it was absent from the isolated child environment. No real provider, MCP, hook or original session was invoked. No Cargo build or source edit was involved.

The published binary reports `grow 2.6.1 (deaca48)` and SHA25653225d8ca9257d0c8d178ce437af784906b69a89daaef9150c204be0b14e8585, matching the already attested/signed public asset. Two local chat-completions requests were observed (one without tools, one with30tools). The actual tool-bearing provider request exports one spawn_subagent with effort type string|null, not required; required is exactlyparent_step_id/prompt/description. Its model/effort descriptions preserve omitted source selections on resume and resume_from states their independent overrides.

Grow exited0 without timeout, the mock thread ended, and no process remained in the isolated group. Parent read the complete probe and bounded result, independently recomputed the binary hash and asserted schema fields/descriptions and clean process status. Evidence: `/tmp/grow-effort-release-probe/{probe.py,result.json}`. This confirms runtime declaration from the published package, not a strings/source inference. It does not claim to re-execute every delegation/effort behavior; existing2.6.1behavior acceptance owns that coverage.

## Disposition

No remaining implementation defect is supported for the screenshot: it reflects the original2.6.0schema, while both2.6.1source and actual package declaration provide the requested parameter. The next-version maintenance change adds only a short version/process diagnostic note to16-subagents.md and updates the tools README's archived task-planning pointer to the canonical spec. It does not duplicate a feature or change product contracts. A resumed investigation attempt failed in host compaction before creating a probe; it was released after checking no process/artifact existed, then a fresh bounded verifier produced the accepted evidence above. User installation and running sessions are untouched.
