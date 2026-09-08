# Why
Permission cache read errors currently return None, the same as NotFound. A client-specific cache containing invalid UTF-8 therefore inherits shared grants, while invalid TOML uses default state.

# What Changes
Only NotFound allows fallback. Other IO/read-decoding failures yield default state and retain the failed source.

# Impact
Remembered grants are not imported from shared storage after a client cache read failure. Independent explicit configuration and permission modes remain unchanged.
