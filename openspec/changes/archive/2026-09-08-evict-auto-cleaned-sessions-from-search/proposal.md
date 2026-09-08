## Why
Automatic session TTL cleanup removes directories without notifying the search projection, unlike manual deletion. Completed-bootstrap markers can leave stale documents visible.

## What Changes
Publish successfully deleted identities to the existing search update/eviction path for the same storage root.
