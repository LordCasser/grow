# Evidence

- mint_provider_token formerly formatted the first 300 characters of captured stderr into its anyhow error. ensure_fresh_token and recover_rejected_token log that returned error via tracing.
- parse_token_output formerly formatted serde_json::Error directly; a malformed expires_in string is included in that error text.
- Both output channels now produce only structural diagnostics: process status, JSON classification and coordinates, and captured stderr byte count. No raw serde error is retained as an error source.
- stdout/stderr collection and caps remain unchanged; the count describes captured bytes, not necessarily all bytes the process emitted. No token-cache or auth-retry logic changed.
