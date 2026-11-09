# Design

The unit test re-executes its exact libtest name before creating any Workflow fixture. `TestSandbox` owns HOME, USERPROFILE, GROW_HOME, temp and workspace paths; its cleared child environment also removes ambient GROW_ROOT/GROW_USER. `TestProcess` owns bounded output, a deadline and process cleanup. An exact-test marker allows only that child to execute the test body. The parent asserts one successful execution, so a stale or empty test selection cannot pass.

The helper uses a separate thread/runtime for subprocess waiting, preserving tests with their own runtime, large stack or paused Tokio clock. The sandbox lives until the owned child is reaped. No global HOME setters or early-startup hooks are used, because shell's existing cold-recovery subprocess fixtures deliberately inherit their own persistent homes.

Validation uses a synthetic user skill larger than the unchanged manifest limit plus a user Agent and workspace-user discovery input. The control proves real discovery sees those inputs before the same test is re-executed through its sandbox. The isolated branch confirms no ambient resources survive and still discovers explicit local skill/Agent fixtures. Full Workflow and adjacent actor regressions run normally and with a polluted outer HOME.

# Platform boundary

The recorded failure and fixed-binary control were on macOS. `dirs` 6 follows HOME on Unix but uses `SHGetKnownFolderPath(FOLDERID_Profile)` on Windows, ignoring USERPROFILE. The adversarial home control is Unix-only; the test subprocess sandbox still owns GROW_HOME and environment on Windows, but no Windows Agent/Skill user-source isolation or native execution is claimed. Changing the production resource-root API is outside this maintenance repair.
