# Design

Treat helper stdout and stderr as credential-bearing output, not safe diagnostic text. Do not attempt substring redaction against only the previous key: helper output may contain a new key unknown to Grow. parse_token_output reports serde classification and source coordinates without Display of the raw serde error. mint_provider_token appends only stderr byte count. Empty/non-UTF8/control-character and process errors retain their existing structural reasons.

Tests invoke a harmless local helper containing a synthetic sentinel, once on stderr with nonzero exit and once as a malformed JSON field. Verify returned errors never contain the sentinel and remain actionable. No real credential/provider commands are invoked.
