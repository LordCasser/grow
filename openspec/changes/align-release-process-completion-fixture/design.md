# Design

The mock explicitly selects completion only after its existing list/ask business calls are done, or when its blocked ordinary foreground answer is released. Both Chat Completions and Responses carry the same final text plus standalone declaration. Inquiry inference remains tool-free. Unique call IDs prevent restored history collisions. Keep every original cancellation, isolation, permissions, reload and recovery assertion. Real native CLI runs and optimized distribution smokes validate integration.

Ordinary Turn requests are recognized by the advertised FinishTurn contract. Tool-free auxiliary requests (for example background titles) retain a plain response and do not set or block on the ordinary foreground latch; inquiry Sidebands remain identified and checked separately. This avoids claiming foreground execution from an unrelated auxiliary request, and avoids attaching a host completion tool where it was not offered.
