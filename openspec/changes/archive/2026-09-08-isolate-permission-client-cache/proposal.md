# Why
Client identifiers are arbitrary ACP metadata strings. Lossy underscore substitution aliases remembered permission files (foo/bar, foo\\bar and foo_bar).

# What Changes
Derive per-client filenames from SHA-256 of exact UTF-8 identifier bytes using the existing dependency. Preserve shared permission.toml fallback. Do not migrate ambiguous legacy per-client files.

# Impact
Old per-client grants require approval again. Client identifiers are namespace labels, not authenticated identities.
