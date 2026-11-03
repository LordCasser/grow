# Gate Unix-only terminal tests

## Why

The terminal login-environment parser is compiled only on Unix, but two unit tests that call it are currently compiled on every target. On Windows this prevents the tools test target from compiling. The Bash `background_check` test module contains only one Unix-specific test, while its unconditional `use super::*` becomes unused on Windows.

## What Changes

Gate the parser tests with the same Unix cfg as the parser, and gate the Bash test module with Unix. Preserve all Unix coverage and all Windows-valid tests. No runtime behavior changes.
