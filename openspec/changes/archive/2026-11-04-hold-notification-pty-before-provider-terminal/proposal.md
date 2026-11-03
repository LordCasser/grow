## Why

Candidate `455bcb92475066e31a361c6c4f6160f33441ecdb` failed Core run 41698917082 in the background-child notification suspension PTY: the notification Turn had ended before the suspended checkpoint assertion. The fixture blocked the last SSE frame, `[DONE]`, after already sending `response.completed`. The sampler correctly treats `response.completed` as terminal, so the fixture did not hold the intended active notification Turn.

## What Changes

- Omit the optional trailing `[DONE]` from the two explicitly blocked Responses scripts in this fixture, so the existing last-event barrier holds `response.completed` itself.
- Keep both normal and Minimal process recovery assertions, child identity, interruption evidence, exactly-once completion and original deadlines.

This changes test data only. The archived session-timeline background-notification recovery and client-surfaces quit contracts remain unchanged; `skip_specs: true` avoids inventing a behavior delta.
