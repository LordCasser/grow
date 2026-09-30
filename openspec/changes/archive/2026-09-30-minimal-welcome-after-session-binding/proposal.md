# Proposal: retain the Minimal welcome card across session binding

## Why

The release PTY case found that a Minimal welcome card can appear at cold start and then disappear before the first turn finishes. A new Agent view starts with no session ID. Minimal's visible owner includes that ID, so session creation changes the owner and clears the current screen. The card was already committed before the ID arrived and is not replayed in the new epoch. A long turn can therefore leave no first card in native history.

## What Changes

Commit the pending card only after the new root session is bound. Keep the pending flag through the placeholder epoch and preserve the existing one-card-per-new-session behavior. The separate PTY-fixture change handles first-turn synchronization.
