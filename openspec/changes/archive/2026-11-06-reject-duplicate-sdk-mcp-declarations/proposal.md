# Reject duplicate SDK MCP declarations

## Why

The SDK MCP metadata parser silently keeps the first same-name registration, contrary to the existing requirement that a single client-provided server array with repeated names is rejected (MCP-F3). Parsing currently happens after session preparation side effects, so simply returning an error there is too late.

## What Changes

Make the existing SDK parser fallible for duplicate parsed canonical names and validate recognized declarations at new/load ingress before lifecycle or persistence effects. Preserve malformed-entry handling and established cross-source precedence; this fix does not combine standard/config/plugin sources into a new override policy or add SDK update support.
