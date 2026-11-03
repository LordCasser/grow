#!/usr/bin/env python3
"""Bounded executable model for passive message delivery (stdlib only)."""
from __future__ import annotations

from collections import deque
from dataclasses import dataclass, replace
import json
from pathlib import Path


IDS = ("m1", "m2")
PAYLOADS = ("same consultation text", "same consultation text")
PHASES = ("idle", "boundary", "request", "tool", "final_decision", "closing", "canceling", "suspended", "crashed")


def bit(i: int) -> int:
    return 1 << i


@dataclass(frozen=True)
class State:
    phase: str = "idle"
    owner: str = "source"                 # source answering Session or fork
    terminal: int = 0                      # new-contract successful terminal proofs
    proof_valid: int = 0                   # binds asker, original target, Sideband, frozen source
    foreign_terminal: int = 0               # invalid proof accepted (must remain zero)
    history_terminal: int = 0               # inherited/legacy observation, never local authority
    received: int = 0                      # durable receipt commits
    received_ack: int = 0                  # ACK confirmed, or authoritative reconciliation
    received_write_failed: int = 0          # attempted write failed before durable commit
    consumed: int = 0                      # durable Consumed + exact input publication
    consumed_ack: int = 0                   # consumption ACK confirmed/reconciled
    input_published: int = 0               # exact model input atomically committed with Consumed
    request_inputs: int = 0                # exact identities in most recent dispatched request
    omitted_required: int = 0               # mutated dispatch omitted frozen safe-boundary input
    dispatched: int = 0                    # identities included in at least one request
    provider_read: int = 0                 # intentionally never inferred from dispatch
    consume_counts: tuple[int, int] = (0, 0)
    boundary_ids: int = 0                  # terminal obligations + Received IDs frozen at safe boundary
    candidate_ids: int = 0                 # IDs frozen for the tentative final-response decision
    candidate_taken: bool = False
    budget: int = 1
    behavior_allows: bool = True
    held_fifo: bool = False
    turn_open: bool = False
    wake: bool = False
    preempt: bool = False                  # explicit user steer / parent interrupt only
    passive_preempt: bool = False
    passive_tool_cancel: bool = False
    permission: int = 0                    # context text never creates permission evidence
    over_budget: bool = False
    cancel_requested: bool = False


def invariant(s: State) -> str | None:
    if s.terminal & ~s.proof_valid:
        return "successful terminal lacks a valid structured source/owner proof"
    if s.foreign_terminal:
        return "foreign or malformed source proof was accepted as a terminal"
    if s.owner == "fork" and (s.terminal or s.received or s.consumed or s.input_published):
        return "fork acquired source-owned delivery authority or pending work"
    if s.received & ~s.terminal:
        return "Received receipt lacks its canonical successful terminal proof"
    if s.received_ack & ~s.received:
        return "receipt ACK confirmed without a durable Received commit"
    if s.consumed != s.input_published:
        return "Consumed and exact input publication were not committed together"
    if s.consumed & ~s.received:
        return "Consumed exists without durable Received"
    if s.consumed_ack & ~s.consumed:
        return "Consumed ACK confirmed without durable Consumed"
    if any(n > 1 for n in s.consume_counts):
        return "stable receipt identity was consumed more than once"
    if any((n == 1) != bool(s.consumed & bit(i)) for i, n in enumerate(s.consume_counts)):
        return "per-identity consumption count disagrees with durable Consumed set"
    if s.request_inputs & ~s.input_published or s.request_inputs & ~s.consumed_ack:
        return "provider request includes input without confirmed durable Consumed"
    if s.omitted_required:
        return "provider request omitted a terminal obligation or receipt frozen at its safe boundary"
    if s.dispatched & ~s.consumed_ack:
        return "dispatch history includes input without confirmed durable Consumed"
    if s.provider_read:
        return "model inferred provider observation from request dispatch"
    if s.passive_preempt or s.passive_tool_cancel:
        return "passive context requested preemption or tool cancellation"
    if s.permission:
        return "consultation prose created human permission evidence"
    if s.wake and s.phase == "idle":
        return "passive context requested a synthetic idle Turn"
    if s.over_budget or s.budget < 0:
        return "continuation was granted without remaining budget"
    return None


def transitions(s: State, mutant: str | None = None):
    """One action per durable/control step; optional mutant changes an enabled action."""
    out = []
    active_writer = s.owner == "source" and s.phase not in ("crashed", "suspended", "closing")
    for i in range(2):
        b = bit(i)
        if active_writer and not (s.terminal & b):
            # Only the correctly bound proof can establish the new terminal.
            out.append((f"terminal_valid({IDS[i]})", replace(s, terminal=s.terminal | b, proof_valid=s.proof_valid | b)))
            if mutant == "malformed_proof_acceptance":
                out.append((f"submit_malformed_proof({IDS[i]})", replace(s, terminal=s.terminal | b, foreign_terminal=s.foreign_terminal | b)))
            else:
                out.append((f"submit_malformed_proof({IDS[i]})", s))
        if s.owner == "fork" and s.history_terminal & b:
            out.append((f"ignore_inherited_terminal_as_local_sync({IDS[i]})", s))
        if active_writer and s.terminal & b and not s.received & b:
            out.append((f"received_write_fails_before_commit({IDS[i]})", replace(s, received_write_failed=s.received_write_failed | b)))
            committed = replace(s, received=s.received | b, received_ack=s.received_ack | b)
            ack_lost = replace(s, received=s.received | b)
            if mutant == "passive_autowake" and s.phase == "idle":
                committed = replace(committed, wake=True); ack_lost = replace(ack_lost, wake=True)
            if mutant == "passive_preemption" and s.phase in ("request", "tool"):
                committed = replace(committed, passive_preempt=True, passive_tool_cancel=(s.phase == "tool"))
                ack_lost = replace(ack_lost, passive_preempt=True, passive_tool_cancel=(s.phase == "tool"))
            if mutant == "permission_from_text":
                committed = replace(committed, permission=committed.received); ack_lost = replace(ack_lost, permission=ack_lost.received)
            out.append((f"commit_received({IDS[i]})", committed))
            out.append((f"commit_received_ack_lost({IDS[i]})", ack_lost))
        if active_writer and s.terminal & b and not s.received & b and s.phase in ("idle", "crashed", "suspended"):
            out.append((f"recover_missing_received({IDS[i]})", replace(s, received=s.received | b, received_ack=s.received_ack | b)))
        if active_writer and s.terminal & b and not s.received & b and s.phase in ("boundary", "request", "tool", "final_decision"):
            out.append((f"reconcile_live_missing_received({IDS[i]})", replace(s, received=s.received | b, received_ack=s.received_ack | b)))
        if active_writer and s.received & b and not s.received_ack & b:
            out.append((f"reconcile_received_ack({IDS[i]})", replace(s, received_ack=s.received_ack | b)))
        if active_writer and s.received & b:
            # Stable ID + same payload replays; changed payload is explicitly rejected.
            out.append((f"retry_same_identity_same_payload({IDS[i]})", s))
            out.append((f"reject_same_identity_conflicting_payload({IDS[i]})", s))
        if mutant == "fork_recreation" and s.owner == "fork" and s.history_terminal & b and not s.received & b:
            out.append((f"recover_missing_received({IDS[i]})", replace(s, received=s.received | b)))

    # Explicit products retain their own control signals and identities.
    if s.phase == "request" and not s.preempt:
        out.append(("admit_user_steer_then_soft_preempt", replace(s, preempt=True)))
        out.append(("commit_parent_interrupt_then_interrupt", replace(s, preempt=True)))

    # Existing idle arbitration gates ordinary work; passive context does not enter this path.
    if s.phase == "idle" and not s.held_fifo:
        out.append(("ordinary_admission_starts_turn", replace(s, phase="boundary", turn_open=True, boundary_ids=(s.terminal | s.received))))
    if s.owner == "source" and s.terminal and s.phase == "idle":
        out.append(("fork_inherits_terminal_as_history_only", replace(s, owner="fork", history_terminal=s.terminal, terminal=0, proof_valid=0, received=0, received_ack=0, consumed=0, consumed_ack=0, input_published=0, request_inputs=0, dispatched=0, consume_counts=(0, 0), turn_open=False)))
    if s.phase == "idle" and s.held_fifo:
        out.append(("held_fifo_denies_idle_admission", s))

    if s.phase == "boundary":
        authorized = s.owner == "source" and s.turn_open and s.budget > 0 and s.behavior_allows
        required = s.boundary_ids & s.terminal & ~s.consumed
        eligible = required & s.received & ~s.consumed
        if s.budget > 0:
            out.append(("budget_revoked_at_boundary", replace(s, budget=0)))
        if s.behavior_allows:
            out.append(("behavior_revoked_at_boundary", replace(s, behavior_allows=False)))
        if s.budget > 0 and s.consumed:
            out.append(("budget_revoked_after_consumed", replace(s, budget=0)))
        if s.behavior_allows and s.consumed:
            out.append(("behavior_revoked_after_consumed", replace(s, behavior_allows=False)))
        for i in range(2):
            b = bit(i)
            if authorized and eligible & b and s.received_ack & b and not s.consumed & b:
                counts = list(s.consume_counts); counts[i] += 1
                out.append((f"commit_consumed_exact_input({IDS[i]})", replace(s, consumed=s.consumed | b, consumed_ack=s.consumed_ack | b, input_published=s.input_published | b, consume_counts=tuple(counts))))
                out.append((f"commit_consumed_ack_lost({IDS[i]})", replace(s, consumed=s.consumed | b, input_published=s.input_published | b, consume_counts=tuple(counts))))
            if s.consumed & b and not s.consumed_ack & b:
                out.append((f"reconcile_consumed_ack({IDS[i]})", replace(s, consumed_ack=s.consumed_ack | b)))
            if mutant == "repeat_consumption" and authorized and s.boundary_ids & b and s.consumed & b:
                counts = list(s.consume_counts); counts[i] += 1
                out.append((f"commit_consumed_exact_input({IDS[i]})", replace(s, consume_counts=tuple(counts))))
        if authorized and required == 0 and (s.input_published & s.consumed_ack) == s.input_published:
            out.append(("dispatch_exact_committed_surface", replace(s, phase="request", request_inputs=s.input_published, dispatched=s.dispatched | s.input_published, budget=s.budget - 1)))
        if mutant == "consume_before_durable_ack" and eligible:
            out.append(("dispatch_exact_committed_surface", replace(s, phase="request", request_inputs=eligible, budget=s.budget - 1)))
        if mutant == "omit_pending_context" and eligible:
            out.append(("dispatch_exact_committed_surface", replace(s, phase="request", request_inputs=s.input_published, omitted_required=eligible, budget=s.budget - 1)))
        if mutant == "bypass_terminal_obligation" and required:
            out.append(("dispatch_exact_committed_surface", replace(s, phase="request", request_inputs=s.input_published, omitted_required=required, budget=s.budget - 1)))
        # A request may proceed with an empty/newly projected Surface when no eligible passive receipt exists.
        if authorized and required == 0 and (s.input_published & s.consumed_ack) == s.input_published:
            out.append(("dispatch_without_pending_context", replace(s, phase="request", request_inputs=s.input_published, dispatched=s.dispatched | s.input_published, budget=s.budget - 1)))
        if not authorized and (s.received & ~s.consumed):
            out.append(("gate_denies_before_consumption", replace(s, phase="idle", turn_open=False)))

    if s.phase == "request":
        out.append(("request_enters_tool_batch", replace(s, phase="tool", request_inputs=0)))
        out.append(("request_produces_final_candidate", replace(s, phase="final_decision", request_inputs=0, candidate_taken=False)))
        out.append(("cancel_exact_owner", replace(s, phase="canceling", turn_open=False, preempt=False, request_inputs=0)))
    if s.phase == "tool":
        # A structured tool batch closes before the next sampling boundary is frozen.
        if s.cancel_requested:
            out.append(("tool_batch_settles_then_canceling", replace(s, phase="canceling", turn_open=False, request_inputs=0)))
        else:
            out.append(("tool_batch_completes_new_safe_boundary", replace(s, phase="boundary", boundary_ids=(s.terminal | s.received), request_inputs=0)))
        if not s.cancel_requested:
            out.append(("cancel_exact_owner_waits_for_tool_settlement", replace(s, cancel_requested=True, turn_open=False)))
    if s.phase == "final_decision" and not s.candidate_taken:
        # Candidate is a snapshot for deciding whether the same Turn may continue.
        pending_at_snapshot = s.terminal & ~s.consumed
        out.append(("snapshot_final_candidate_ids", replace(s, candidate_ids=pending_at_snapshot, candidate_taken=True)))
    if s.phase == "final_decision":
        out.append(("cancel_exact_owner", replace(s, phase="canceling", turn_open=False, candidate_ids=0)))
    if s.phase == "final_decision" and s.candidate_taken:
        can_continue = bool(s.candidate_ids) and s.owner == "source" and s.turn_open and s.budget > 0 and s.behavior_allows
        if can_continue:
            out.append(("candidate_authorizes_same_turn_successor", replace(s, phase="boundary", boundary_ids=(s.terminal | s.received), candidate_ids=0, candidate_taken=False)))
        if not can_continue:
            out.append(("irreversible_final_close", replace(s, phase="closing", turn_open=False)))
        if mutant == "continuation_without_budget" and s.candidate_ids and s.budget == 0 and s.turn_open and s.behavior_allows:
            out.append(("candidate_authorizes_same_turn_successor", replace(s, phase="boundary", boundary_ids=(s.terminal | s.received), candidate_ids=0, over_budget=True)))
    if s.phase == "closing":
        out.append(("close_to_idle_no_wake", replace(s, phase="idle", turn_open=False, wake=False)))
    if s.phase == "canceling":
        out.append(("cancel_finishes_to_idle", replace(s, phase="idle", turn_open=False)))
    if s.phase not in ("suspended", "crashed", "closing"):
        out.append(("suspend_preserving_pending", replace(s, phase="suspended", turn_open=False, request_inputs=0)))
        out.append(("crash", replace(s, phase="crashed", turn_open=False, request_inputs=0)))
    if s.phase in ("suspended", "crashed"):
        out.append(("restore_without_synthetic_turn", replace(s, phase="idle", turn_open=False, wake=False, preempt=False)))
    return out


def replay(start: State, actions: list[str], mutant: str | None = None) -> State:
    state = start
    for action in actions:
        matches = [n for label, n in transitions(state, mutant) if label == action]
        if len(matches) != 1:
            raise AssertionError(f"trace action not uniquely enabled: {action}; phase={state.phase}")
        state = matches[0]
        bad = invariant(state)
        if bad:
            raise AssertionError(f"trace violates {bad} after {action}")
    return state


def initial_configurations():
    starts = []
    for phase in PHASES:
        for owner in ("source", "fork"):
            for held in (False, True):
                for budget in (0, 1):
                    for behavior in (False, True):
                        # A fork may display a historical success but owns no new proof or receipt.
                        history = 1 if owner == "fork" else 0
                        starts.append(State(phase=phase, owner=owner, held_fifo=held, budget=budget, behavior_allows=behavior, history_terminal=history, turn_open=phase in ("boundary", "request", "tool", "final_decision")))
    return starts


def explore(starts, depth=7, mutant=None):
    queue = deque((s, []) for s in starts)
    seen = {s for s in starts}
    edges = 0
    actions_seen = set()
    while queue:
        s, trace = queue.popleft()
        bad = invariant(s)
        if bad:
            return {"states": len(seen), "transitions_checked": edges, "actions_reached": sorted(actions_seen), "counterexample": {"invariant": bad, "trace": trace}}
        if len(trace) >= depth:
            continue
        for action, nxt in transitions(s, mutant):
            edges += 1
            actions_seen.add(action.split("(")[0])
            bad = invariant(nxt)
            if bad:
                return {"states": len(seen), "transitions_checked": edges, "actions_reached": sorted(actions_seen), "counterexample": {"invariant": bad, "trace": trace + [action]}}
            if nxt not in seen:
                seen.add(nxt)
                queue.append((nxt, trace + [action]))
    return {"states": len(seen), "transitions_checked": edges, "actions_reached": sorted(actions_seen), "counterexample": None}


def find_mutant(mutant: str, start: State, depth=10):
    result = explore([start], depth=depth, mutant=mutant)
    if result["counterexample"] is None:
        raise AssertionError(f"mutation escaped detection: {mutant}")
    return result["counterexample"]


def scenarios():
    results = []
    # Full durable receipt -> safe consume -> request dispatch sequence is transition-driven.
    s = replay(State(phase="request", budget=2, turn_open=True), [
        "terminal_valid(m1)", "commit_received(m1)", "request_enters_tool_batch",
        "tool_batch_completes_new_safe_boundary", "commit_consumed_exact_input(m1)",
        "dispatch_exact_committed_surface",
    ])
    assert s.request_inputs == 1 and s.dispatched == 1 and s.provider_read == 0
    results.append({"name": "terminal_received_consumed_then_dispatched", "trace": ["terminal_valid(m1)", "commit_received(m1)", "request_enters_tool_batch", "tool_batch_completes_new_safe_boundary", "commit_consumed_exact_input(m1)", "dispatch_exact_committed_surface"]})

    # Identical prose is distinct under two identities. Same-ID same payload replays; conflict rejects.
    require(PAYLOADS[0] == PAYLOADS[1] and IDS[0] != IDS[1], "same-text distinct-ID fixture")
    s = replay(State(), ["terminal_valid(m1)", "commit_received(m1)", "retry_same_identity_same_payload(m1)", "terminal_valid(m2)", "commit_received(m2)", "reject_same_identity_conflicting_payload(m2)"])
    assert s.received == 3
    results.append({"name": "identity_replay_conflict_and_same_text_distinct_ids", "trace": ["terminal_valid(m1)", "commit_received(m1)", "retry_same_identity_same_payload(m1)", "terminal_valid(m2)", "commit_received(m2)", "reject_same_identity_conflicting_payload(m2)"]})

    # Failure before commit differs from ACK lost after commit and is reconciled in place.
    s = replay(State(), ["terminal_valid(m1)", "received_write_fails_before_commit(m1)"])
    assert s.received == 0 and s.received_write_failed == 1
    lost = replay(State(), ["terminal_valid(m1)", "commit_received_ack_lost(m1)", "reconcile_received_ack(m1)"])
    assert lost.received == 1 and lost.received_ack == 1
    results.append({"name": "precommit_failure_vs_commit_ack_lost", "trace": ["terminal_valid(m1)", "received_write_fails_before_commit(m1)", "commit_received_ack_lost(m1)", "reconcile_received_ack(m1)"]})

    # Recovery derives only from the canonical terminal, never re-running the sideband.
    s = replay(State(phase="crashed", terminal=1, proof_valid=1), ["restore_without_synthetic_turn", "recover_missing_received(m1)"])
    assert s.received == 1 and s.terminal == 1
    results.append({"name": "replacement_writer_recovers_missing_receipt", "trace": ["restore_without_synthetic_turn", "recover_missing_received(m1)"]})

    # A canonical terminal is already an obligation at boundary entry, even if Received is missing.
    frontier = replay(State(budget=2), ["terminal_valid(m1)", "ordinary_admission_starts_turn"])
    assert not any(a in ("dispatch_exact_committed_surface", "dispatch_without_pending_context") for a, _ in transitions(frontier))
    frontier = replay(frontier, ["reconcile_live_missing_received(m1)", "commit_consumed_exact_input(m1)", "dispatch_exact_committed_surface"])
    assert frontier.dispatched == 1
    results.append({"name": "boundary_frontier_reconciles_terminal_before_dispatch", "trace": ["terminal_valid(m1)", "ordinary admission freezes terminal obligation", "dispatch denied while Received missing", "live reconciliation", "Consumed", "dispatch"]})

    # Cutoff is two-stage: m1 enables successor; m2 arrives after that snapshot and is consumed at next safe boundary.
    s = replay(State(phase="final_decision", terminal=1, proof_valid=1, received=1, received_ack=1, budget=2, turn_open=True), [
        "snapshot_final_candidate_ids", "candidate_authorizes_same_turn_successor",
        "terminal_valid(m2)", "commit_received(m2)",
        "commit_consumed_exact_input(m1)", "dispatch_exact_committed_surface",
        "request_enters_tool_batch", "tool_batch_completes_new_safe_boundary",
        "commit_consumed_exact_input(m2)",
    ])
    assert s.consumed == 3
    results.append({"name": "candidate_snapshot_then_later_receipt_joins_next_boundary", "trace": ["snapshot_final_candidate_ids(m1)", "candidate_authorizes_same_turn_successor", "Received(m2) after snapshot", "consume_and_dispatch(m1)", "tool batch", "next safe boundary", "consume(m2)"]})

    # If no candidate succeeds, irreversible final close wins; later receipt remains pending without wake.
    s = replay(State(phase="final_decision", budget=0, turn_open=True), ["snapshot_final_candidate_ids", "irreversible_final_close", "close_to_idle_no_wake", "terminal_valid(m1)", "commit_received(m1)"])
    assert s.received == 1 and s.consumed == 0 and not s.wake
    results.append({"name": "irreversible_close_then_late_receipt_waits", "trace": ["snapshot_final_candidate_ids(empty)", "irreversible_final_close", "close_to_idle_no_wake", "commit_received(m1)"]})

    # Denied budget/behavior and held idle FIFO do not consume or wake.
    denied = replay(State(phase="boundary", owner="source", turn_open=True, budget=0, behavior_allows=True, terminal=1, proof_valid=1, received=1, received_ack=1, boundary_ids=1), ["gate_denies_before_consumption"])
    held = replay(State(phase="idle", held_fifo=True, received=1, terminal=1, proof_valid=1, received_ack=1), ["held_fifo_denies_idle_admission"])
    assert denied.consumed == held.consumed == 0 and held.phase == "idle"
    results.append({"name": "budget_and_fifo_gates", "trace": ["budget=0 boundary -> gate_denies_before_consumption", "held FIFO idle -> deny ordinary admission"]})

    # Runtime gate revocation is a transition, both before and after the durable consume commit.
    revoked_before = replay(State(budget=2), ["ordinary_admission_starts_turn", "dispatch_without_pending_context", "terminal_valid(m1)", "commit_received(m1)", "request_enters_tool_batch", "tool_batch_completes_new_safe_boundary", "behavior_revoked_at_boundary", "gate_denies_before_consumption"])
    assert revoked_before.received == 1 and revoked_before.consumed == 0
    revoked_after = replay(State(budget=2), ["ordinary_admission_starts_turn", "dispatch_without_pending_context", "terminal_valid(m1)", "commit_received(m1)", "request_enters_tool_batch", "tool_batch_completes_new_safe_boundary", "commit_consumed_exact_input(m1)", "behavior_revoked_after_consumed"])
    assert revoked_after.consumed == 1 and revoked_after.input_published == 1
    assert not any(a == "dispatch_exact_committed_surface" for a, _ in transitions(revoked_after))
    results.append({"name": "behavior_revocation_before_and_after_consumed", "trace": ["revoke before consume -> pending receipt retained", "separate trace: commit Consumed + exact input", "revoke before dispatch -> publication remains but request is denied"]})

    # Consumed ACK unknown blocks dispatch; authoritative reconciliation enables it without consuming twice.
    s = replay(State(phase="boundary", owner="source", turn_open=True, budget=1, terminal=1, proof_valid=1, received=1, received_ack=1, boundary_ids=1), ["commit_consumed_ack_lost(m1)"])
    assert not any(a == "dispatch_exact_committed_surface" for a, _ in transitions(s))
    s = replay(s, ["reconcile_consumed_ack(m1)", "dispatch_exact_committed_surface"])
    assert s.consume_counts == (1, 0)
    results.append({"name": "consumed_ack_lost_blocks_dispatch_until_reconciled", "trace": ["commit_consumed_ack_lost(m1)", "dispatch denied", "reconcile_consumed_ack(m1)", "dispatch_exact_committed_surface"]})

    # A successful final candidate authorizes a successor, but uncertain Consumed ACK still blocks dispatch.
    s = replay(State(budget=2), ["ordinary_admission_starts_turn", "dispatch_without_pending_context", "terminal_valid(m1)", "commit_received(m1)", "request_produces_final_candidate", "snapshot_final_candidate_ids", "candidate_authorizes_same_turn_successor", "commit_consumed_ack_lost(m1)"])
    assert not any(a == "dispatch_exact_committed_surface" for a, _ in transitions(s))
    s = replay(s, ["reconcile_consumed_ack(m1)", "dispatch_exact_committed_surface"])
    results.append({"name": "candidate_successor_still_waits_for_consumed_ack", "trace": ["snapshot m1 candidate", "authorize same-Turn successor", "commit Consumed with ACK lost", "dispatch denied", "reconcile", "dispatch"]})

    # Tool cancellation is cooperative; final-decision cancellation supersedes a candidate continuation.
    tool_cancel = replay(State(budget=2), ["ordinary_admission_starts_turn", "dispatch_without_pending_context", "request_enters_tool_batch", "terminal_valid(m1)", "commit_received(m1)", "cancel_exact_owner_waits_for_tool_settlement", "tool_batch_settles_then_canceling", "cancel_finishes_to_idle"])
    assert tool_cancel.phase == "idle" and tool_cancel.received == 1 and tool_cancel.consumed == 0
    candidate_cancel = replay(State(budget=2), ["ordinary_admission_starts_turn", "dispatch_without_pending_context", "terminal_valid(m1)", "commit_received(m1)", "request_produces_final_candidate", "snapshot_final_candidate_ids", "cancel_exact_owner", "cancel_finishes_to_idle"])
    assert candidate_cancel.phase == "idle" and candidate_cancel.received == 1 and candidate_cancel.consumed == 0
    results.append({"name": "tool_cancel_settles_cooperatively_and_final_cancel_wins", "trace": ["request -> tool", "request exact-owner cancellation", "tool settles -> canceling", "separate final-candidate trace: snapshot -> cancel -> idle, receipt pending"]})

    # Pending survives cancellation/suspend. Restore alone remains idle. Fork history is inert.
    s = replay(State(phase="request", terminal=1, proof_valid=1, received=1, received_ack=1, turn_open=True), ["cancel_exact_owner", "cancel_finishes_to_idle", "suspend_preserving_pending", "restore_without_synthetic_turn"])
    assert s.received == 1 and s.consumed == 0 and s.phase == "idle" and not s.wake
    f = replay(State(owner="fork", history_terminal=1), ["ignore_inherited_terminal_as_local_sync(m1)"])
    assert f.received == 0 and f.terminal == 0
    results.append({"name": "cancel_suspend_restore_and_fork_history", "trace": ["pending receipt", "cancel", "suspend", "restore to idle without wake", "fork inherited history has no local receipt"]})

    # A valid explicit control remains distinct and tool batches complete before a new boundary.
    s = replay(State(phase="request"), ["admit_user_steer_then_soft_preempt", "request_enters_tool_batch", "tool_batch_completes_new_safe_boundary"])
    assert s.phase == "boundary"
    results.append({"name": "explicit_steer_and_tool_batch_boundary", "trace": ["explicit steer -> soft preempt", "request", "tool batch completes", "safe boundary"]})

    # Same transition relation includes source proof validation and no permission path.
    replay(State(), ["submit_malformed_proof(m1)"])
    results.append({"name": "malformed_proof_rejected_and_text_is_not_permission", "trace": ["submit_malformed_proof(m1)", "consultation payload retained as evidence only"]})

    # This single fair witness is a positive progress trace under an eligible boundary.
    witness = replay(State(phase="request", budget=2, turn_open=True), ["terminal_valid(m1)", "commit_received(m1)", "request_enters_tool_batch", "tool_batch_completes_new_safe_boundary", "commit_consumed_exact_input(m1)", "dispatch_exact_committed_surface"])
    assert witness.dispatched & 1
    results.append({"name": "fair_eligible_boundary_drain_witness", "trace": ["terminal", "Received", "safe boundary", "Consumed", "dispatch"]})
    return results


def require(name, condition):
    if not condition:
        raise AssertionError(name)


def mutation_counterexamples():
    return {
        "passive_autowake": find_mutant("passive_autowake", State()),
        "passive_preemption_or_tool_cancel": find_mutant("passive_preemption", State(budget=2)),
        "dispatch_before_consumed_ack": find_mutant("consume_before_durable_ack", State(budget=3)),
        "omit_boundary_eligible_context": find_mutant("omit_pending_context", State(budget=3)),
        "bypass_unreceived_terminal_obligation": find_mutant("bypass_terminal_obligation", State()),
        "fork_recreation": find_mutant("fork_recreation", State()),
        "continuation_without_budget": find_mutant("continuation_without_budget", State()),
        "repeat_consumption": find_mutant("repeat_consumption", State(budget=3)),
        "malformed_proof_acceptance": find_mutant("malformed_proof_acceptance", State()),
        "permission_from_consultation_text": find_mutant("permission_from_text", State()),
    }


def main():
    starts = initial_configurations()
    bounded = explore(starts, depth=7)
    require("correct model bounded exploration", bounded["counterexample"] is None)
    named = scenarios()
    mutants = mutation_counterexamples()
    report = {
        "model": "passive-message-delivery bounded transition system",
        "scope": "message proof, durable receipt/consumption, execution boundary, request dispatch and recovery",
        "exploration": {"max_depth": 7, "initial_configurations": len(starts), **bounded},
        "named_scenarios": named,
        "mutation_counterexamples": mutants,
        "model_findings_for_plan": [
            "An authorized boundary freezes both new-contract successful terminal obligations and already-Received IDs. Dispatch waits for every frozen obligation to reach Received, Consumed, and confirmed ACK; an existing terminal with missing Received cannot be skipped.",
            "A tentative final-response candidate snapshot and irreversible final close are distinct transitions. If m1 is in the candidate snapshot and authorizes same-Turn continuation, m2 received afterward can join a later normal safe boundary; if close wins with no eligible candidate, later receipts remain pending without a wake.",
            "The candidate snapshot is not a durable minimum-turn latch. Receipt identities are selected at ordinary safe boundaries and remain subject to owner, budget, Behavior and cancellation gates.",
            "Consumed ACK uncertainty blocks dispatch until authoritative reconciliation confirms the durable commit; it does not justify consuming again.",
            "Runtime budget/Behavior revocation before Consumed leaves the obligation pending. Revocation after Consumed preserves the committed Surface but prevents dispatch under the closed owner. Cancellation during a tool batch waits for the batch to settle; cancellation during final decision defeats candidate continuation."
        ],
        "limitations": [
            "This model does not prove the Rust implementation, filesystem persistence, actor integration, provider behavior or multi-writer linearizability.",
            "Exploration covers 144 enumerated starting configurations to depth 7 and two distinct identities with identical payload text; it makes no infinite-liveness claim.",
            "The state machine abstracts timing, payload bytes, actual journal encoding and provider internals.",
            "Only one explicit fair finishing trace demonstrates drain progress when an eligible boundary is scheduled."
        ]
    }
    Path(__file__).with_name("results.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps({"exploration": {"initial_configurations": len(starts), "states": bounded["states"], "transitions_checked": bounded["transitions_checked"], "actions_reached": len(bounded["actions_reached"]), "counterexample": bounded["counterexample"]}, "scenario_count": len(named), "mutation_count": len(mutants)}, indent=2))


if __name__ == "__main__":
    main()
