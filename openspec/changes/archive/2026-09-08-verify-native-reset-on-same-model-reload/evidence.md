# Evidence

- apply_user_model_selection distinguishes real model changes from effort-only or identical selection. Its route_changed comparison must not be treated as the catalog-reload predicate.
- apply_model_config_reload first stages workflow route and durably records the model transition, then always calls ChatState::replace_sampling_route, including unchanged ModelId.
- ChatState ReplaceSamplingRoute clears the continuation lane before assigning config. UpdateSamplingConfig deliberately preserves it.
- The regression seeds signed Messages continuation, verifies it is present, reloads each transport variation under unchanged ModelId, then checks absence of native continuation and preservation of portable history plus the new config.
