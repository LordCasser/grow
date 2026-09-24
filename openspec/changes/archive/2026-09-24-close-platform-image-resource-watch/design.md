# Design

The relevant existing boundaries are distinct and should not be conflated:

- macOS explicit image reads run through an owned `osascript` subprocess with a five-second deadline, bounded stdout/stderr and process-group cleanup. Its private transfer files inherit a 50,000,000-byte per-file `RLIMIT_FSIZE`; Grow reads at most 50,000,001 bytes and rejects data over 50,000,000 bytes. These limits contain process lifetime, file growth, and Grow's subsequent read. They do not limit helper/AppKit RSS, aggregate temporary storage, or native image decoding.
- Non-macOS arboard image read and PNG encoding hold a single process-wide worker permit. The caller waits at most two seconds, but a timed-out worker retains the permit until it exits. Once arboard returns, Grow rejects RGBA over 16,000,000 pixels and caps PNG output at 50,000,000 bytes. Linux CLI capture is capped at 50,000,000 bytes. None of these caps bounds backend allocations made before RGBA is returned.

A five-second process deadline is not an RSS limit: ending an over-time child bounds duration, not its peak memory before termination. `RLIMIT_RSS` is not a reliable portable enforcement mechanism for these native backends and does not provide a defensible hard memory ceiling. Replacing these clipboard backends or introducing OS-level process/container isolation would be platform-specific, substantially broader work without evidence of a current resource incident or a supported low-cost Grow-owned allocation to cap. The one-worker admission, deadlines, and post-return byte/pixel caps already bound Grow-controlled concurrency and subsequent allocations.

Close this watch while stating the residual plainly: a helper or platform backend may still consume substantial memory before completion or before returning RGBA. Reopen with target-platform peak measurements and a concrete supported enforcement mechanism if that risk becomes actionable. This decision does not claim a hard bound on OS-owned memory.
