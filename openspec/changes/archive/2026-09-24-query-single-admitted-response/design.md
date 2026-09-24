# Design

The Timeline already owns branch selection and response admission identity. Expose a query that verifies the identity's event is selected by the branch fold and clones only that event's response. Route it through the existing ChatState actor query/oneshot pattern. Replace the live Shell's `timeline_events`/`Timeline::from_events`/`admitted_responses` sequence with this query. Cold replay continues to rebuild canonical projections from its validated Timeline.
