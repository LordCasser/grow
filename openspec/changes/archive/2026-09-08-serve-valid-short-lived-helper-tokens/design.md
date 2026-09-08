## Design
Keep the existing skew predicate for proactive minting and rejected-token recovery. Cache-only reads require matching token identity and actual unexpired lifetime. On failed pre-turn mint, mark the retained token expired, as the 401 failure path already does, so it remains helper context only. No new cache or expiry field.

## Verification
A local synthetic helper returns a 30-second token. Assert the request-time bearer resolver serves it after mint, then rejects actual expiry and a changed config. A counting helper succeeds once then fails; a near-expiry refresh failure must withdraw the prior token. Run the auth test group.
