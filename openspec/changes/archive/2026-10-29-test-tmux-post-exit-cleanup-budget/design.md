## Design

Factor the existing post-leader-exit process-group cleanup and shared pipe-drain deadline into one helper. The regular bounded runner passes its process deadline to the helper as context, but the helper starts the existing 300 ms post-exit drain allowance. A focused Unix test starts a shell leader and a Perl descendant that inherits the output pipes. The descendant installs its TERM handler, writes the expected output, and then signals readiness; the shell leader waits for that readiness before exiting. The test confirms the descendant is alive, invokes the same cleanup helper with an expired process deadline, and checks captured output plus a marker written by the TERM handler before it exits. The test does not assert elapsed wall-clock durations or claim to reap the orphaned descendant directly.

No production timeout or process-group behavior changes. `.openspec.yaml` uses `skip_specs: true` because only test coverage and internal factoring change.
