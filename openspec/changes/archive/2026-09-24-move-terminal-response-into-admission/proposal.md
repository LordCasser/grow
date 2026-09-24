# Move terminal response ownership into admission

## Why

The turn path takes ownership of a terminal response's items and native continuation, then clones both into the durable Timeline admission even though the original owned values are only used for a post-admission assistant count. A large terminal payload therefore has an avoidable second live allocation at the preview-to-canonical boundary.

## What Changes

Count assistant items before admission and transfer the owned response items and native continuation directly to the admission API. This changes no contract, persisted format, response content, or admission order, so no delta spec is needed.
