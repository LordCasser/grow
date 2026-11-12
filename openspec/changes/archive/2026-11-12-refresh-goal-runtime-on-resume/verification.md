# Verification

The reported session journal showed execution release after cold restore, followed by a successful /goal restart. The original Goal became Active with no plan waits. No autonomous turn started before the user's next ordinary message. The runtime was built with a false Goal availability flag and gated startup deferred the refresh. The release path reopened execution without running it.

The new PTY test parks a Goal turn on the mock provider, pauses the Goal, confirms suspension, starts a new process with --continue and restarts the Goal. It checks for a new persisted goal_continuation and completion without another user prompt.

The v2.6.7 CLI passed both real PTY checks. The Goal count went from one continuation before restart to two after restart, and the new continuation completed via update_goal. The test waits for composer readiness and actual input echo before submitting restart. The existing process test for Goal sampling failure, explicit restart and budget gates also passed against the candidate CLI.

The pre-fix CLI reached the same successful restart notice and displayed Goal: Active. It started no continuation during the 120-second observation window. The same compiled test passed against v2.6.7, with one new Goal-owned continuation and a durable Complete state.
