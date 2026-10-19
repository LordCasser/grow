# Design

## Recovery boundary

The three-attempt malformed-generation limit belongs to one logical sampling request. Exhaustion rejects the candidate, ends its Step and Turn, clears the Turn/sampling continuation, and releases the exact foreground owner through ordinary completion settlement. A later user prompt is a newly admitted Turn in the same actor; it gets a new logical sampling budget and uses the valid historical projection. Durable usage and task/Goal budgets are not request-local counters and must not reset.

Malformed protocol JSON/SSE and invalid tool arguments remain different error classes. This change does not broaden automatic retry eligibility. Rejected candidate calls and their valid siblings never execute or enter native continuation.

## Child output grant

The attempt usage sink owns accounting: known output debits the grant once; unknown usage or a failed ledger write marks the grant incomplete and closes it. The outer unconditional `fail_task_output_usage_closed` call after sampler errors is redundant with accounted sampling and falsely exhausts precisely settled grants. Remove that call and its unused helper; retain the authoritative unknown/failed-settlement paths. Test exact known usage, insufficient remaining output and unknown usage. No new grant is minted by sending a prompt.

A normal sampler terminal follows attempt settlement. If its ordered event does not reach the stream-drained acknowledgement barrier, the session cannot prove that every admitted attempt settled. Keep conservative grant closure at that specific unconfirmed-lifecycle boundary. Earlier known ledger entries stay intact; this does not create another provider call to repair accounting.

## Goal behavior

An exhausted Goal-internal Turn follows the existing durable pause path. Provider recovery alone and ordinary user text do not silently reactivate that Goal. A user may interact with the resident session while the Goal is paused or budget-limited. Such explicitly admitted user Turns are outside the stopped Goal and cannot alter its old spending or resolve its uncertainty. The existing explicit Goal restart path is responsible for activation; restart retains identity and spending and rejects unresolved exact-budget uncertainty or an exhausted budget. Child output grants remain session-scoped and are still enforced for manual input; Goal lifecycle and child grants are different authorities.

## Test boundary

Use real `MvpAgent`/session commands and production session threads rather than calling `handle_prompt` directly. Each failure/recovery pair keeps its session handle and actor identity. A loopback provider scripts three complete bad-argument responses and then healthy output. Assert the failed Turn is terminal before a successor, new input can also queue while settlement is pending, no invalid tool enters execution/history, attempt accounting remains exact, and the restored provider is called with a fresh request budget. Cover Chat Completions, Responses and Messages. Child and Goal tests use the closest existing production entry points and explicitly document their limits.

Binary tests run in isolated temporary Grow homes to avoid process-global config and real user data. They negotiate `samplingAttemptLifecycle` explicitly: the append-only client retains its no-retry boundary for observed output, while the retractable client exercises the three-attempt cap. The budgeted-child-context test drives the production session command loop and actual ChatState event channel, with controlled Timeline acknowledgements. It holds the first failed Turn terminal, submits `继续`, verifies no provider dispatch before ACK, then checks ordinary settlement and successor ordering. Its persistence is simulated; the binary regressions exercise the actual disk writer. The existing core gate executes both the library and exact built-binary selections on the release-selected commit; no separate recovery framework or workflow is needed.
