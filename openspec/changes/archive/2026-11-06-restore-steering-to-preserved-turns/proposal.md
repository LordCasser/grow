# Restore steering to preserved turns

## Why

Pending-input reconstruction unconditionally reroutes admitted Steer inputs to FIFO. A valid suspended checkpoint keeps the target Turn active, so the canonical validator correctly rejects that reroute and prevents cold session restoration (F1).

## What Changes

Consult the canonical runtime checkpoint during input reconstruction. Rebuild an unconsumed input bound to the preserved turn in the existing interjection buffer with its original identity and content; retain the normal FIFO reroute only after interruption recovery has closed the target. Keep the Timeline reroute validator and existing restore release barrier unchanged. FIFO replacement position/identity (F2) remains separate.
