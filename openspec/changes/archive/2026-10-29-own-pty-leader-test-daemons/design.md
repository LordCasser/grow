## Design

`LeaderCluster` will retain a `test_support::process::TestProcess` created from its existing `ContentController` `TestSandbox` and resolved pager binary. The owner starts the dedicated `--no-exit-on-disconnect` daemon, waits for its isolated socket to appear, and uses the existing bounded process-group kill/reap cleanup in `Drop`. The owner is the first cluster field, so it drops before the sandbox and mock server. Clients use the same isolated `leader.sock` path. No production process discovery, lock-file PID signaling, or generic cleanup framework is introduced.

The existing `TestProcess` panic-drop regression exercises cleanup during unwinding; ordinary scope exit uses that same `Drop`. No archived behavior contract changes; `.openspec.yaml` uses `skip_specs: true`.

真实binary回归直接观察TestProcess持有的daemon PID；普通作用域结束及catch_unwind后的waitid均返回ECHILD，证明直接子进程已回收，不依赖目录删除或socket消失代替进程终止。
