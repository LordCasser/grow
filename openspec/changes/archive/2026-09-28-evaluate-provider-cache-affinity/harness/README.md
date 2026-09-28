# Bounded provider cache experiment harness

This standard-library-only harness does not open sockets, invoke a provider SDK, read a session snapshot, or inspect environment credentials. The default `mock` mode reads only the committed synthetic fixture and its per-route budget file. Its output is synthetic bookkeeping, not a cache measurement.

The fixture explicitly includes the three prerequisite evidence shapes: stable image projection selection, cache usage field availability, and offline request-pair diagnostics. These are synthetic representations of those changes' outputs. They check that an experiment record can preserve the relevant facts; they do not replace prerequisite implementation verification or the offline diagnostics tool.

Run the offline example from the repository root:

```sh
python3 openspec/changes/evaluate-provider-cache-affinity/harness/experiment_harness.py
python3 -m unittest discover -s openspec/changes/evaluate-provider-cache-affinity/harness -p 'test_*.py' -v
```

`mock_budgets.json` fixes each candidate's request count, per-request output tokens, aggregate output tokens, and aggregate cost ceiling. The harness refuses missing budgets, records only attempts within every applicable ceiling, and reports the first budget that stops further attempts. Costs are fixture values used to test stopping behavior; they are not provider prices.

`--mode plan` is a preparation boundary, not a sender. It requires `--route-config` with `confirmed: true`, explicit provider/model/backend/deployment/region/tenant label, and all four positive budget fields. It emits no credentials, and rejects secret-bearing field names. The output always says `network_requests_sent: 0` and `execution_status: plan_only_sender_not_implemented`. There is no real-send mode because this harness has no reviewed sender that can preserve and enforce the same evidence and budgets across Responses, Chat Completions, and Messages. Actual provider experiments remain manual and unexecuted here.

Example route configuration shape (replace placeholders only after independently confirming the route):

```json
{
  "provider": "confirmed-provider-id",
  "model": "exact-model-version",
  "backend": "responses|chat_completions|messages",
  "deployment": "confirmed-deployment",
  "region": "confirmed-region-or-explicit-unknown-label",
  "tenant_label": "non-secret-isolation-label",
  "confirmed": true,
  "budget": {
    "max_requests": 8,
    "max_output_tokens_per_request": 256,
    "max_total_output_tokens": 2048,
    "max_cost_usd": "1.00"
  }
}
```

Do not place keys, authorization data, or secret headers/query parameters in the configuration. This plan mode does not authorize or execute real requests.
