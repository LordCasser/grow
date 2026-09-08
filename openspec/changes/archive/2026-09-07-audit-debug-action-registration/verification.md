# Registration audit

- slash/commands/mod.rs unconditionally registers DebugCommand alongside ScrollDebugCommand. DebugCommand::visible uses cfg!(debug_assertions), while run still maps empty/scroll/fps/log to real actions. Release-hidden is not release-disabled; retain these supported diagnostics.
- actions::ActionId is a typed enum with no string/serde decoding. Default registry construction consumes defaults::default_actions; no DumpInputLog ActionDef or custom construction was found across repository sources.
- All ActionId::DumpInputLog references are the enum declaration, the agent view's return-None branch, and dashboard's unsupported-action match. None emits a runtime Action.
- Actual input dumping uses app::actions::Action::DumpInputLog, produced directly by the Esc-then-d input handler and handled by dispatch_dump_input_log. This is a distinct type and remains functional independent of ActionId.
- R14 proposes removing only the unused ActionId variant and its exhaustive no-op match arms. Actual Action, chord, recorder, file writer, tests and slash debug features must remain. Code has not been removed.

## Limits and validation
Source/reference audit only, not a release binary execution test. No claim that repository-external consumers never construct the public ActionId variant. No behavioral tests or CLI build needed/run for this documentation-only change. OpenSpec strict validation and git diff --check used for the records.
