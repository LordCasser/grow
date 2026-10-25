## Decision

Cold ChatState starts with a new continuation lane, so its initial message contribution must use the same provider-neutral portable replay as that lane. Reuse `sampling_types::project_portable_history` and the existing message token estimator. Do not restore a prior live provider anchor as if it measured the new replay, or clip the result to the window.

`projected_request_surface_tokens` must still retain the raw Surface estimate. `projected_request_input_tokens` and `projected_tokens` start at the portable message estimate. This preserves the existing signed Surface delta stream and lets the first real request replace the initial projection adjustment with its current tools, schemas, Goal and image envelope. No new persistence, provider call or initialization request is needed.

The current load path publishes fresh notifications and `/context` reads the same ChatState counter. Fixing the owner therefore updates resumed display and skill admission together, without a separate Pager counter.
