## Decision

ChatState owns `projected_tokens`: final request projection adjusts it for provider-visible content and envelopes, provider observations anchor it, and committed Surface mutations apply signed estimate deltas. Ordinary context pressure and conditional recall commits already use this value.

Both skill publication methods must use that same current value for existing context, then add the estimate of their complete new items. Do not rebuild context pressure from raw Surface, change token estimators, loosen the reserved budget, or add a second accounting state. The existing dependent request build still applies its final projection and budget checks.

## Verification boundaries

Exercise raw history larger than the window with a fitting projected request, and a small raw history whose provider anchor exhausts headroom. Rejections must preserve the user's unconsumed group and close model calls using the existing fallback; accepted publication must add its full content delta once.
