# Evidence and limits
Source inspection on main, 2026-09-08:

- agent/mvp_agent/acp_agent.rs passes summary.current_model_id into SessionSpawnOptions, but does not pass summary.reasoning_effort.
- agent/mvp_agent/agent_ops.rs spawn_and_register_session resolves sampling_config from that model's current configuration.
- session/actor/spawn.rs seeds SessionModelRoute from that sampling_config.
- acp_agent.rs subsequently enqueues summary.reasoning_effort as a model-switch metadata override, and discards restore errors.
- agent/handlers/model_switch.rs prepares the default route then applies the requested effort override.
- session/actor/model_switch.rs commit_model_change uses the actor route as from, and commits when effort differs even without a client intent.
- session/persistence.rs latest_model_selection rejects any prior to/current from mismatch, including effort; jsonl reconcile_model_projection calls it before repairing summary.

A read-only walk of local timeline.jsonl files found 3 discontinuous model observations: seq 77 after 19; seq 1099 after 1078; seq 144 after 17 (different sessions). All previous to efforts were high and subsequent from efforts max. This corroborates the shape, but does not prove which binary or action produced those records. No matching model event at seq 622 was found locally. No session content or credentials are copied into this record, and no user history was altered.

workflow's handle_tool_parse_error emits a failed tool update and pushes its tool result. It does not directly commit a model change. The screenshot's temporal ordering is not yet a proven causal connection.

Atlas scoped search located commit_model_change but returned a stale signature/line; current source was used as authority rather than the stale symbol position. No Rust test or build has yet been run for this change; the source-derived reproduction remains to be established with an isolated test.
