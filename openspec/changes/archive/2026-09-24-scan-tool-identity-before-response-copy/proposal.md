# Scan tool identity before copying a response

## Why

The response admission path copies the entire current Surface and response to run malformed-tool quarantine, then discards that copy for valid responses. This is a large avoidable peak during ordinary sampling.

## What Changes

Use a read-only identity scan to select the existing repair path only when needed. This is an internal allocation refactor. The malformed identity policy, Timeline admission, quarantine count and repair events remain unchanged, so no contract delta is introduced.
