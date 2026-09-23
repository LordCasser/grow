## Evidence
One full terminal image run returned19passed/1failed at normal-exit runner unwrap with Os code1 PermissionDenied. Prior19-test run passed. The newly added output-growth case also passed in the failing run.

A separate Python experiment waited/reaped100 new-session exit0 children then sent group SIGKILL and signal0: all100 yielded ESRCH/ESRCH. This is a different spawn implementation, not proof about the Rust runner.

Temporary stage instrumentation was added to Rust spawn, attach and non-ESRCH cleanup exits. The normal-exit test expanded to200 alternating exit0/exit7 commands passed; the complete20-test module with that expansion also passed. No diagnostic error occurred, so the originating stage remains unknown. All temporary instrumentation and repetition were removed afterward. No EPERM suppression or speculative retry was added.

## Follow-up bounded experiment
Added temporary stage-aware errors and ran4 parallel workers with500 exit0 children each through the actual runner. All2000 completed without errors in10.40s. This still does not identify the stage or cause. The diagnostic harness was removed.

attribute-sips-process-errors now preserves stage names in ordinary runner errors. Injected startup EPERM proves startup failures can share the same error code; it does not identify the original event. That separate diagnostics change passed22 module tests. Root-cause attribution remains pending; do not resolve this investigation from green repeats alone.
