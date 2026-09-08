## Why
A background title failure can restore the route after a manual title command cleared it, causing later user input to launch a redundant auto-title request. Canonical title protection prevents overwrite but does not prevent this request.

## What Changes
Keep title-route revocation authoritative while a worker holds the route, without disabling legitimate transient-failure retry.
