# Evidence
- schedule_session_title takes Option<SessionTitleRoute>, leaving None while the background task owns the route.
- SetSessionTitle takes the same slot before committing the user title. During an in-flight title task this removes nothing; None cannot distinguish in-flight ownership from permanent revocation.
- generate_session_title restores Some(route) on begin/attempt/admission/usage/terminal-record failures. These await points permit a manual rename before the restoration.
- Ordering: task claims route -> manual title command takes empty slot and commits -> old task encounters a restorable error -> replaces slot with Some(route) -> next user prompt can schedule another title request.
- Timeline rejects Generated/Fallback after User title, preserving the visible title. The issue is unnecessary provider work and a one-shot capability revived after revocation, not proven title overwrite.
- A simple check before an awaited restore is insufficient if rename can interleave after that check. The route slot needs to preserve revocation while a worker owns the actual route, or use an equivalent serialized authority mechanism.

# Repair boundary
Represent ready/in-flight/revoked ownership explicitly within the existing title route lifecycle; a failed worker may return its route only while its claim is still eligible. Preserve ordinary retry after transient setup failure and canonical manual-title priority. Do not add an independent persistent title store.
