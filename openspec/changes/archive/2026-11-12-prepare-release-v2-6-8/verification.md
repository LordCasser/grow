# Verification

The Pager library test filter `force_quit` passed both tests on the v2.6.8 candidate. The dispatch test checks that force quit does nothing outside exit suspension and quits in both pending and unconfirmed states. The presentation test checks that the pending notice exposes the action and the recovery warning.

The v2.6.7 CLI passed both real PTY regressions for interrupted command duration and Goal restart. This candidate changes the force-quit test and version metadata. The official workflow runs the full gates against the new tag before publication.
