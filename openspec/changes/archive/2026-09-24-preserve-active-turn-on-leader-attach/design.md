# Design

The owner boundary is `MvpAgent::load_session`: it knows whether it just claimed a new writer (`spawn_new_actor`) or is reconnecting to a resident actor. Both cases reconcile subagent projections, but only the new incarnation may close process-local interrupted scopes after that reconciliation. Pass this owner fact into the existing reconciliation function and gate its final `recover_interrupted_durably` call. The session actor's startup recovery remains unchanged.

Keep the completed-turn replay tests' intended order by waiting for the existing durable `turn_completed` record after the first streamed answer. Add a separate paced stream scenario that attaches a viewer while the first turn is provably active, waits for its real terminal, then submits a later turn and checks both panes. This catches the previous unconditional Recovery terminal without conflating active-stream attach with completed-history replay.
