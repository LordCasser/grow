## Why

The v2.2.0 release matrix cannot compile either Windows target with pinned Rust 1.93.1. The local draft quarantine guard calls unstable Windows `MetadataExt` file identity methods. The archived client-surfaces contract already requires the active path to be checked against the opened source on every platform.

## What Changes

- Compare Windows file identities using stable handles from the existing `same-file` crate.
- Preserve the existing missing-path and replacement behavior, and leave Unix identity checks unchanged.

This repairs compilation and implements the existing contract on Windows; it does not change the contract, so no spec delta is needed.
