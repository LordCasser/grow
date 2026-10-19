# Unify received agent communication rows

## Problem

Outgoing agent messages and inquiries already render as communication tool rows, but a durably received message or reply is projected as a generic notice. This makes the two sides of one explicit communication use different transcript surfaces and leaves received messages without the same operation-row layout and details affordance.

## Change

Project only structurally valid `AgentMessageNotice` receipts as ordinary communication tool rows. Use the stable Timeline receipt ID for deduplication, label the row as a received message or reply from its structured source, and keep the immutable received state terminal. Preserve the full body and protocol data. Malformed or identity-less notices remain finite notices.

## Scope

The shared Pager notice projector covers live, replayed and child-session history. Minimal mode continues to print each immutable receipt once. This does not alter message delivery, consumption, model context, inquiry lifecycle, or delivery authority.
