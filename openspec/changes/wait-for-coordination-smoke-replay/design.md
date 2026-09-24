## Approach

Reuse the fixture's existing `eventually` helper at the notification-read boundary. Keep the cursor (`before`) captured before each `session/load`; wait only for matching inquiry id and `inquiry completed` subject from that load, then assert exactly one terminal and its `target_restarted` audit. The 15-second default is already used by this fixture for asynchronous coordination observations.
