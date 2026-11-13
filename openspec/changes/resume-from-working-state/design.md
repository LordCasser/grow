## Adversarial findings

- A full serialization of the current Timeline is not a working-state checkpoint. Timeline owns the event vector and retained identity maps, and callers use history for provider request ownership, rewind provenance, input and Hook settlement, child results, and model context. Separate cold lookups before making the compact state an execution source.
- Journal physical positions include presentation records, while domain event sequences do not. A checkpoint binds both coordinates, the storage identity, and the branch. An offset, a file length, and a cached prefix digest alone cannot establish that a checkpoint still describes the same committed journal.
- If the checkpoint or required lookup data was not committed when a process stopped, the next writer must recover from the last independently valid publication. The journal can contain a complete record whose ACK was lost. Retrying must retain exact identity and bytes. A torn tail cannot replace a complete record or authorize execution.
- Rewind can change the selected branch without discarding raw evidence. A page cursor must bind the chosen branch and fixed cut. A physical suffix alone loses earlier anchors, command/tool grouping, and pending responses. New pages need the corresponding canonical facts and dependencies.
- Historical controls and lifecycle observations cannot update current runtime controls when a page is prepended. Native terminal scrollback is append-only. A page insertion must go through a view that can place and navigate earlier records without corrupting already committed native lines.
- A logically closed child is not necessarily settled. Result, parent terminal, usage, planning receipt, and notification can commit separately. Recovery must identify the outstanding stages rather than infer completeness from one flag or a displayed row.
- A single logical Turn may be large even after compaction. Counting old steps and copying old tool payloads in the active Turn can still make exit depend on history. Retain the exact active cursor and evidence consumed by suspension.
- Existing sessions need an explicit first full validation to build trustworthy state and indexes. Measure that migration separately from repeat resume. Preserve the existing ledger and active work.
- Checkpoint and indexed readers must keep the existing writer lease and contained file semantics. Cold artifact integrity is checked when accessed; exact evidence used by live recovery is checked before release. An old corrupt artifact outside the execution dependency set need not block unrelated resume.
- A/B must fix active state and vary closed history. Appending uncompressed messages to the current Surface changes the independent variable. Capture the complete suspend acknowledgement, cold load completion, input readiness and latest-page visibility as distinct endpoints.

## Approach

1. Move suspension and child reconciliation to facts owned by the validated actor. Reduce repeated folds first and preserve existing storage validation during the transition.
2. Persist a working-state projection and indexed cold references at a committed frontier. Hydrate before accepting commands, verify execution dependencies, and replay only the durable tail. Keep exact historical deduplication and arbitrary-range operations available without materializing the closed prefix.
3. Load the latest display page from an indexed branch-bound history range. Use distinct older-page and live-delivery positions. Prepend old pages without executing their controls or shifting the viewport anchor. Complete export, search, and rewind continue to consume complete verified history when invoked.

Use the same source facts throughout. The checkpoint and indexes can be rebuilt from the journal. Persisted index or checkpoint mismatches must never supply an empty or partial authority.

### Commit and recovery boundaries

Session creation establishes the initial published cut. Ordinary durable appends advance it from the previous cut under the writer lease. Sync canonical bytes first, then the new references and working-state changes, then publish the manifest. A lost acknowledgement continues to use the exact journal identity. Exit settles outstanding work and confirms its writes using the same commit path. Work between publications must be bounded by new commits, including after abrupt process loss.

The executable checkpoint contains the compacted model Surface with its original IDs and revision, current control, model selection, Goal and usage aggregates, the open Turn/Step and waits, and pending input, Hook, child and workflow obligations. Historical identities still used for deduplication, authorization or provenance remain addressable by exact indexed lookups. Timeline must stop using historical vector offsets as an execution-state API. Keeping old facts in the executable fold preserves correctness during the transition but does not meet this contract.

The presentation reader uses the selected branch and a fixed published cut. It resolves recent records and the dependencies of each requested page without rebuilding that branch's complete display projection. A presentation result cannot authorize execution, change a live cursor or release ownership. Export and audit can consume the complete canonical stream independently.

An existing monolithic journal without a checkpoint has no bounded way to prove that an old record does not contain an unresolved obligation or a branch transition. Rebuild it before direct use when possible, under a pinned cut and with normal publication ordering. If directly opened before migration, execution must wait for verified state while navigation remains available. Measure this separately and keep the measured 26-second direct-open path visible as an outstanding migration issue.

### Dependencies to remove from cold execution

| Current consumer | Needed before execution | Historical access |
| --- | --- | --- |
| Timeline and model request assembly | Selected Surface, IDs, revision, pending Control activation, active route and required sampling evidence | Earlier branch and admission sources by exact ID, including those required by rewind or permission provenance |
| Input, request, tool, and Hook admission | Current owner, attempt, causal links and pending settlement | Closed identities and terminal outcomes by exact key. For example, a PostToolUse Hook must still prove the call started and is closed |
| Runtime, Goal, child, and Workflow recovery | Open owners, unsettled delivery stages, and current usage and budget state | Completed execution and usage details for diagnostics and transcript |
| History projection | A published cut and the live delivery boundary | Rows by branch and page, expanding to include response anchors and complete tool groups |

Each commit must update the affected projection entries under the same publication. The equivalence check compares the restored Surface, model request, obligations and next identities with dense replay. An exact read must distinguish absence from read or checksum failure. Treat a missing historical key as absent only when the selected index covers that keyspace and frontier. Keep user-visible history loading outside the response that releases the reconciled runtime. Associate asynchronous page delivery with the original session binding and merge it before the fixed live-delivery position.

The existing loader also validates sampling audit bodies from every historical request, even though those bodies do not restore native provider continuation. Preserve schema and reference validation at publication. Verify artifact bytes when a consumer requests them or runs a full transfer or audit, and preserve synchronous verification for any artifact that actually participates in execution. Planning and control revisions require current snapshots and exact references for outstanding approvals, rewind and previous decision identities.

### First disk-backed slice

Publish an immutable index of checksummed raw record references under the writer lease. Its manifest binds the session, source inode, verified physical frontier and index file identity. Keep independently projectable standalone notices separate from records that contribute to the Timeline fold, append identity checks, or branch selection. Cold load reads the latter and applies the new journal tail with the existing validation. Recent and older pages merge reference-backed notices with their ordered canonical projection. Export and unsupported branch shapes retain full validation. No index row owns a copy of response text.

Standalone notices carrying only the normal event ID and emission timestamp also qualify. A sorted identity table in the same immutable index stores the ID hash and physical position. An append or tail replay probes that table and resolves any hash match against the exact source record before deciding identity equality. The manifest carries the presentation counter high-water mark. The full journal remains authoritative for both page contents and lost-acknowledgement identity checks.

A growing tail is replayed from the published cut without rebuilding the indexed prefix. New sessions publish the initial base in the staged directory. Ordinary appends advance the manifest after 1 MiB of new journal data, syncing buffered canonical bytes if necessary. Exit can use the same bounded publication path if its final writes cross the threshold. The immutable base remains available to earlier page cursors. Post-base standalone notices are still retained for append deduplication and display.

This slice bounds work attributable to indexed standalone notices. The domain fold and historical identity indexes still require their own working-state representation to meet the full scaling contract. Measure the two cases independently and leave the change active until both pass.

## Increment status

Domain sequence is independent of the number of resident facts. A runtime-neutral diagnostic observation can leave the executable fold after its original journal record is validated. Such records still occupy physical and domain positions. The journal reader used by trajectory and export retains complete history. For a checkpoint, distinguish display-only rows, runtime-neutral domain records, and facts with executable dependencies. Domain sequence continuity and the append receipt survive holes in the resident facts.

The event tracker emits runtime-neutral diagnostics under the `shell` observation scope. The immutable domain position table provides exact disk reads for those facts. The actor uses resident events for coordination recovery and reads complete canonical history explicitly when requested. The first diagnostic record still contributes the usage epoch timestamp. Original event numbers remain the coordinates used by Surface, Sideband, and runtime ownership.

The generation also owns fixed-width domain and retained-record reference tables. Advancement appends only newly committed references and replaces a fixed-size manifest. A reader uses the last published counts, and a writer replaces interrupted entries beyond them from the canonical journal. Explicit domain history reads seek published entries and scan only the unpublished journal suffix. The retained table still grows with execution-bearing facts and post-base presentation. Hydrating those records remains the outstanding cost.

An unchanged-length journal whose modification stamp differs from the published stamp cannot be treated as an append to that publication. Cold writer admission returns to canonical validation in this case. Requested historical pages continue to validate their selected record digests. With the current single mutable JSONL file, observing both a prefix rewrite and file growth requires stronger prefix provenance than the file stamp provides.

The first increment removed repeated parent folding and narrowed live suspension reads. The next increment folds the current Turn's Step, Stop, consumed input and open tool evidence as commits arrive. Pager no longer holds confirmed-session exit behind the read-only prompt history request. Shell delivers the recent page for opted-in loads and older pages through a passive cursor route. Each older request verifies its fixed journal cut on a blocking worker; full export takes the independent validated history path.

Standalone notices with standard presentation metadata have a disk-backed fast path after a full indexed base has been published. Shell reads the referenced retained records and the physical tail, then checks the same domain and artifact invariants. Page reads address the omitted notices by physical reference and verify the exact record digest before display. Leader reconnect still uses its per-subscriber full replay contract. Rewind falls back to the complete projector.

The storage change must move closed identity lookups off the resident Timeline before serializing the working state. The current fold keeps the raw event vector, historical input and Hook lifecycles, seen request/step/tool identities, and exact replay references. Even without the event bodies, serializing those maps at Ctrl+D grows with closed work. The indexed prefix uses individual record digests. Its manifest and the immutable index bind the retained projection to the same journal inode and cut. The writer checks its epoch stamp before extending that prefix. Domain replay, historical child lifecycle copies and candidate cloning for unfinished Hooks remain on the active path. The current ACP load also awaits recent-page replay before returning the successful load response. The working-set checkpoint, post-readiness presentation protocol, and pre-existing session migration remain open.

## Verification

Use isolated test homes and the same build profile, fixture generator, settings and launch paths for A and B. Compare fixed working state with at least three closed-history sizes. Record wall-clock distributions, bytes and records read or copied, pages delivered, and peak memory where available. Include a crash before and after durable checkpoint publication, a partial tail, a lost acknowledgement, a competing writer, a child with a pending result delivery, and an older page completing during a live update or session change. Measure the actual terminal paths as well as storage work so UI and owner retirement are observable independently.
