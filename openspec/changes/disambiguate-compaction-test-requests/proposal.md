## Why
Final Linux CI 34229667703 timed out in async_compaction_promoted_job_is_cancelled_by_control_transition before promotion began. The shared fixture registers its blocked summary and both foreground responses with the same auxiliary matcher. A single-tool foreground request without attribution headers is classified as auxiliary and can consume the blocked summary response when request arrival order changes.

## What Changes
Give this test agent two stable tools and use foreground matchers for foreground responses, preserving the existing mock classifier and all timeouts/barriers. No production behavior changes.
