# Design

Keep the existing response metadata extraction and `persisted_items` count. Before calling `push_response_durably_with_identity`, count assistant items in the owned vector and record the same number of assistant-message signals. Pass both owned fields by value. No later turn code reads either field. Keep admission error handling and all subsequent Timeline-derived projection logic unchanged.
