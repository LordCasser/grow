# Design

Use the existing `tool_identity_issue` predicate to scan the current Surface followed by incoming items, preserving cross-history duplicate-id detection and order. Stop when the first malformed assistant call is found. Only then materialize the combined candidate and invoke the existing quarantine transform to obtain its exact count and repaired Surface. For an idempotent response replay, scan the current Surface before cloning it for any repair. A clean admission or replay does not allocate a combined conversation.
