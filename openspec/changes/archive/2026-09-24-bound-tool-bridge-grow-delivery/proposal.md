# Bound tool bridge Grow delivery

## Why

The tool notification bridge can copy a completed background task's retained output file (up to 64 MiB) into a Grow `TaskCompleted` projection and enqueue further copies in unbounded persistence and gateway channels. Monitor events can likewise continue to enqueue large live Grow payloads while a gateway consumer stalls. These producers bypass the session preview byte budget during sampling.

## What Changes

Keep full task output in the task output file and canonical model notification, while making the TaskCompleted Grow projection metadata-only. Route payload-bearing bridge Grow delivery for background tasks, scheduled tasks, and monitors through the session's shared preview credits, holding credits through persistence and gateway completion. Preserve durable task-completion acknowledgement before the UI projection when requested.
