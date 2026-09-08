# FPS surface audit

## Working portions
FpsHud records up to 120 Duration samples, refreshes formatted mean/percentile data at most every 250ms, and clears old samples on toggle. Disabled record returns without storing data. Mean reciprocal is labeled fps, explicitly intended as render-cost throughput rather than observed paint frequency. Percentiles linearly interpolate the sorted sample list.

## Confirmed reachability gap
AppView::draw checks screen_mode.is_minimal and calls hooks.draw then returns before fps_hud.overlay and fps_frame_started. Only the ordinary draw path calls fps_hud.record. Repository search finds no FPS call in pager-minimal. /debug fps still toggles the same state in all modes, and /debug status reports that state, so minimal can report on with neither samples nor visible readout.

## Measurement and documentation limits
The writer abstraction queues bytes to a background thread on flush. Timing draw_frame therefore does not measure completed PTY output or terminal paint latency. Existing comments describe a separate dev FrameMetrics overlay, yet no frame_metrics source is present, HONORS_GROW_FPS_ENV is literal true and dev_fps_rows returns constant zero. These comments must be corrected with the implementation work, not used as evidence for a second working profiler.

## Repair boundary
Integrate minimal timing and readout through the existing minimal hook/API and live viewport, preserving on-demand frames, cursor placement and viewport restoration. Include empty/disabled state, enabling/disabling and actual visible layout tests. Do not add a periodic render loop solely to make a diagnostic number move. Retain the throughput-vs-paint distinction; initial audit does not claim a runtime screenshot or benchmark.

## Validation
Source audit only, no behavior edits or tests/build run. Existing FPS unit tests were read for scope, not reported as newly executed. OpenSpec and git diff --check validate these records.
