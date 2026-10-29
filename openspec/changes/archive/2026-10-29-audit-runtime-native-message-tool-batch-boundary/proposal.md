## Why

The archived adapter fixture `native assistant calls[a,b] → result a → AgentMessage → result b` demonstrates a possible projection ordering failure. Its production reachability remained unproven, so the backlog tracked it as a candidate rather than a reproduced defect.

## What Changes

Trace notification admission, active-turn drains, and multi-tool completion to determine whether production can place an AgentMessage between results of one native tool batch. This is a read-only reachability audit; it does not change projection behavior or claim coverage for arbitrary adapter histories.

`.openspec.yaml` uses `skip_specs: true` because this audit changes no behavior contract.
