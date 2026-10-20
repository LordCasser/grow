# Repair third-party build diagnostics

## Why

v2.4.1 release logs exposed real sqlite-vec control-flow defects alongside portability diagnostics, musl/OHOS nix alias deprecations, and a yanked leaf in the independently installed cross tool's lock. Suppressing categories or resolving all install dependencies again would hide errors or enlarge the dependency change.

## What Changes

Maintain a documented minimal sqlite-vec source patch: correct missing-IN selection, reject unsupported vector/metric dispatch, define absent primary-key length, express byte-size comparisons in their actual domain, replace compiler-specific folding pragmas with comments and mark intentionally unused parameters. Remove the C warning suppression. Normalize nix microsecond accessor/arithmetic values to i64 without changing libc-owned timeval layout. Keep cross at official 0.2.5, installing the official x86_64 Linux archive with a fixed independently recorded SHA-256 instead of compiling its old lock.

No dependency upgrade, MCP edit, version bump, release dispatch or public asset/tag modification. Existing release platform catalog uses x86_64 Ubuntu runners for all cross builds. Other unrelated backlog items remain deferred.
