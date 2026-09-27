# Design

Add `[ui].show_model_provider` as an optional boolean defaulting to false, registered under Settings → Models as "Show model provider". This is a presentation preference; it does not change model selection or sampling. The Settings dispatcher updates the pager's existing process-wide render cache optimistically, persists through the existing setting effect, and rolls the cache back on persistence failure. Startup primes the cache from effective configuration.

Both session and Dashboard prompt rendering choose between the catalog's existing display name and the active canonical model ID. The pending Dashboard selection uses its selected model ID. Effort remains the session's effective effort and is appended in either mode. Missing model IDs fall back to the existing display name. The setting affects prompt footer labels only, not Tasks rows, `/usage`, `/model`, or persisted model identity.
