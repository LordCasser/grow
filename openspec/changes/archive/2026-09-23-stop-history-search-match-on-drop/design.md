# Design

The daemon and its worker share an `AtomicBool`. Drop stores `true` with Release ordering before submitting Stop. The matching and highlight collection loops load it with Acquire ordering before and after each per-item operation; when set, they return without sorting further, publishing a snapshot, or beginning another item. The capacity-one wake channel and pending-message Stop precedence remain unchanged.

The cancellation boundary is one nucleo score or one highlight-index computation. Those calls are synchronous and cannot be interrupted internally, so this change guarantees cooperative exit after the current operation rather than a wall-clock deadline. A deterministic unit test holds one operation at a barrier, drops the daemon, verifies Drop returns, releases the operation, and verifies the worker observes cancellation and exits.
