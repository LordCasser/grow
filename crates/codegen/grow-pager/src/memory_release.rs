//! Allocator memory-release seam.
//!
//! Resuming a large session parses the whole `updates.jsonl` and stages the
//! replay in memory — a multi-hundred-MB transient for big sessions. The
//! allocations are freed once replay completes, but with jemalloc the freed
//! pages stay attached to the process (macOS additionally counts `MADV_FREE`d
//! pages in RSS until real memory pressure), so an idle, just-resumed TUI
//! looks like it is "using" several times its live heap.
//!
//! The pager library cannot reference jemalloc (the allocator choice belongs
//! to the composition-root binary — see `grow-pager-bin/src/main.rs`), so
//! the binary installs a release hook here at startup, mirroring the
//! `minimal_hook` IoC seam. The hook purges retained arena pages
//! (`arena.<ALL>.purge`); calling it right after heavy transients drop returns
//! that memory to the OS instead of letting it linger for the process
//! lifetime.
//!
//! Absent a hook (tests, alternate binaries, non-jemalloc builds) everything
//! here is inert.
//!
//! ## Covered memory cliffs
//!
//! [`release_retained_memory`] is invoked after every known multi-MB drop:
//!
//! - session-load replay completion (`dispatch/session/load.rs`) — the parsed
//!   `updates.jsonl` staging transient (100s of MB for long sessions)
//! - reconnect-reload window finalize/abort/supersede
//!   (`agent_view/session.rs`) — gated on `apply_reload_outcome` reporting a
//!   heavy drop: the stashed pre-reload `ScrollbackState` (full-replay
//!   success) or the discarded staging (failure). The common cursor-resolve
//!   outcome reuses the stash and does NOT purge.
//! - subagent transcript replay (`app/subagent.rs`,
//!   `replay_inherited_updates`) — covers both producers (eager live-spawn
//!   and resume-deferred first open), gated on a non-empty replay
//! - closing an agent tab (`dispatch/session/modal.rs`) — the whole
//!   `AgentView` incl. scrollback, render caches, and child views
//! - image viewer close (`media.rs`) — the decoded overlay image
//! - rewind truncation (`dispatch/rewind.rs`) — the removed transcript tail
//!
//! Every purge is edge-triggered by a rare lifecycle/user event and gated on
//! an actual drop, never per frame; draw/tick-path cliffs defer the purge to
//! the post-flush gap so it cannot stall the frame being painted.

use std::sync::OnceLock;

static RELEASE_HOOK: OnceLock<fn()> = OnceLock::new();

/// Install the allocator release hook. Idempotent; first caller wins.
/// Called once by the composition-root binary before the app runs.
pub fn install_release_hook(hook: fn()) {
    let _ = RELEASE_HOOK.set(hook);
}

/// Ask the allocator to return freed-but-retained pages to the OS.
///
/// Cheap enough to call after any known memory cliff (see the module docs for
/// the covered sites). No-op when no hook is installed.
pub fn release_retained_memory() {
    release_retained_memory_with("unattributed");
}

/// [`release_retained_memory`] with memory-cliff attribution: `reason` tags
/// the purge event in the memory trace (`memory_trace` module) so per-site
/// purge frequency, duration, and released footprint are analyzable offline.
/// Use a short stable kebab-case tag (e.g. `"session-load-replay"`).
pub fn release_retained_memory_with(reason: &'static str) {
    let hook = RELEASE_HOOK.get();
    // Skip gauge sampling entirely when tracing is off (`GROW_MEMTRACE=0`
    // or no sink): a disabled trace must add zero syscalls to purges.
    let trace = crate::memory_trace::is_active();
    let before = if trace {
        // Same gauge precedence as the trace's threshold logic: physical
        // footprint where available (macOS), else RSS (Linux) — so purge
        // deltas are computable on every platform.
        let mem = crate::memory_trace::sample_process_memory();
        mem.footprint_bytes.or(mem.rss_bytes)
    } else {
        None
    };
    let started = std::time::Instant::now();
    if let Some(hook) = hook {
        hook();
    }
    if trace {
        crate::memory_trace::record_purge(reason, hook.is_some(), before, started.elapsed());
    }
}

/// Test support: a counting release hook with a **per-thread** counter.
///
/// The real `RELEASE_HOOK` is a process-global `OnceLock`, but dispatch/view
/// code always calls [`release_retained_memory`] on the calling thread, so a
/// thread-local count lets parallel `cargo test` threads assert both positive
/// ("this path released") and negative ("this path must not release") deltas
/// without cross-test interference.
#[cfg(test)]
pub(crate) mod test_support {
    use std::cell::Cell;

    thread_local! {
        static CALLS: Cell<usize> = const { Cell::new(0) };
    }

    fn counting_hook() {
        CALLS.with(|c| c.set(c.get() + 1));
    }

    /// Install the counting hook (idempotent; first caller wins, and every
    /// test installs the same `fn`, so ordering does not matter).
    pub(crate) fn install_counting_hook() {
        super::install_release_hook(counting_hook);
    }

    /// Number of releases observed **on this thread**.
    pub(crate) fn calls() -> usize {
        CALLS.with(|c| c.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_invokes_installed_hook_per_call() {
        // The hook slot is a global OnceLock shared with sibling tests, but
        // the counter is thread-local, so deltas on THIS thread are exact.
        test_support::install_counting_hook();
        let before = test_support::calls();
        release_retained_memory();
        release_retained_memory();
        assert_eq!(
            test_support::calls(),
            before + 2,
            "each release must invoke the installed hook exactly once"
        );
    }
}
