# Design

Use a temporary test actor and the existing apply_model_config_reload entry. Establish a Messages route, durably admit visible assistant facts with a signed native fragment, then reload the same ModelId with a changed endpoint, query route or wire model. Assert the precondition really contains native continuation; afterwards native data is absent, while visible history is preserved. No real provider requests or configuration edits.
