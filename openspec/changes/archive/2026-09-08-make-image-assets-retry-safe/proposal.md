## Why
Repeated image preparation previously created fresh UUID assets after every retry. A lost Timeline acknowledgement cannot safely justify deleting them because the committed message may already reference those paths.

## What Changes
Publish ordered normalized images as one immutable content-derived batch directory. Verify existing bytes before reuse. Prepare new batches privately and publish via existing no-replace directory rename; clean only uncommitted staging. No catalog or acknowledgement-driven deletion. Old UUID assets remain unchanged.
