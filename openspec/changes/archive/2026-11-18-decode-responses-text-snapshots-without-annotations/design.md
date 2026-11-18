## Design

The Responses SSE decoder owns normalization. Select the exact output-text locations in known content-part, output-item done and terminal events. Retain the existing empty-text requirement for the initial part. Reuse the raw JSON member insertion used by terminal usage completion so the SDK still observes duplicate and malformed fields. Apply both defaults when a terminal omits fields from both locations. Native continuation and completion receive the regular typed SDK result. Original evidence still comes from the HTTP body before decoding.
