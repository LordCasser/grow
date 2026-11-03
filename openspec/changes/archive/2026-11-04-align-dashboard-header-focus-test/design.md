## Design

The Dashboard header renderer derives the green style from the conjunction of `list_focused` and `new_agent_button_focused`. `DashboardState::new()` starts with the dispatch input focused; `focus_new_agent_button()` changes the list cursor target but intentionally does not transfer keyboard focus. The test will represent both states directly: list focus plus button cursor for green, and input focus plus the same cursor target for gray. The existing selected-row case continues to verify gray styling. A fixed GrowNight theme makes the foreground colors deterministic even if the parent process exports `NO_COLOR` or a monochrome `TERM`; an explicit inequality assertion ensures that the two expected styles are distinguishable.

This updates only a render test fixture and its comments. No production behavior or archived contract changes.
