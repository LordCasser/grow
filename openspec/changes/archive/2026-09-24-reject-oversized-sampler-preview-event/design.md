## Decision

Keep `credit_cost` as the existing release accounting for accepted events. Add a checked producer-side cost calculation from the same event payload fields. An event with more than `PreviewEventBudget::MAX_BYTES` charged bytes returns a typed local attempt failure before acquiring permits or sending. Control and terminal events retain their existing uncharged path. This is a narrow invariant at the producer, independent of the HTTP evidence limit.
