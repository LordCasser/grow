# Design

Treat the backlog's remaining sentence as an unbounded sampling watch rather than an active defect. The three archived changes supply distinct evidence:

- `throttle-resume-replay-paint` compares the 128/512-turn synthetic PTY fixture before and after replay-only automatic paint throttling. Final history visibility improved from 3,046/9,354 ms to 2,352/6,108 ms; render calls fell from 112/253 to 47/71; key-echo p95 was 7.7/7.8 ms.
- `measure-resume-input-during-replay` exercised 20 key echoes while marked replay was still active with a 40 ms frame-writer delay. It passed at 77.6 ms p95 and 78.2 ms maximum, and the draft survived the loaded boundary.
- `add-dense-tool-resume-pty-probe` exercised a 512-turn, 31,097,749-byte synthetic history with 512 tool calls and updates, the same writer delay, and input during the loading window. All 20 echoes preceded the final marker; p95 was 52.9 ms, maximum 58.5 ms, and the draft survived `SessionLoaded`.

These measurements support closing the generic item because the previously identified replay-paint mechanism was repaired and the measured input cases pass the 100 ms p95 criterion. They do not establish end-to-end latency for real long text, resident/cursor sessions, multiple subagents, other terminal implementations, or arbitrary slow writers. The dense-tool run's first streamed marker was not visible during the echo window, so it measures loading-period responsiveness rather than post-marker replay repaint. Do not infer a universal p95 guarantee from these fixtures.

Reopen a focused backlog item if a specific workload reproduces input p95 above 100 ms or an attributable resume phase remains materially slow; record the fixture, terminal, session shape, and phase evidence so a bounded fix can be selected.

The old Pager watch heading had no remaining entries, so remove the heading and its explanatory boilerplate in the same documentation cleanup.
