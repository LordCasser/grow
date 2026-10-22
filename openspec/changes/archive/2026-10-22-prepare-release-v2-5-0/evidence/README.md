# Evidence

`results.json` records every invocation, including failures and reruns, with its actual logfile and clean source commit. Historical raw log names gain `.gz` under logs/. Distinct retry logfiles preserve the initial failures; `coverage.json` selects the final successful jobs rather than summing retries. Core/target summaries exclude nested test subprocesses.

The 52 selected exact cases comprise 30 CI selections, 18 supplemental selections and four test-support-feature selections. Each must report exactly one non-ignored pass. They are incremental fixture-commit runs against the same signed product candidate; this is not a claim that every case ran on the last fixture commit.

`identity.json` contains only a whitelisted environment, toolchain and probe parameters. The private full `env.json` is deliberately excluded and deleted with the temporary run directory. `run_exact_reusing_compile.py` is a local execution record; it reuses only verified test executables within this fixed run and leaves original enumeration, execution, timeouts and cleanup intact. It is not an installed tool or CI change.

`trace-index.json` distinguishes the five retained modern normal/slow reports. Casts/JSONL and successful image preview screens are compressed in pty-traces/ and image-artifacts/. These correctness samples are not an A/B speed claim.

`binary.json` describes the retained product binary and pinned embedded ripgrep; `candidate-source-review.json` explains its unchanged production input across later fixture commits. `cleanup.json` and `release-process-audit.json` record generated-cache and isolated test-Leader cleanup. The final documentation-only commit does not rebuild the candidate.
