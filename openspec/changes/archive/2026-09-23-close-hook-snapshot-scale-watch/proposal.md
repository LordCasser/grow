# Close conditional Hook snapshot scale watch

## Why

The archived Hook snapshot audit measured the current load/reconnect publication path and found no established latency or traffic budget violation. Its remaining backlog text is conditional on future session scale; it does not describe a current defect with an actionable acceptance threshold.

## What changes

Remove that conditional observation from the unresolved backlog. Keep the archived audit's measurements and ordering limits as the evidence to revisit if real usage crosses a defined budget.

This is backlog maintenance only. It changes no runtime behavior or accepted contract, so `skip_specs: true` is intentional.
