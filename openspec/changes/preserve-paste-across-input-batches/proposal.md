## Why
Unbracketed multiline paste is collected in bounded input passes. The 5,000-event extension cap currently commits an incomplete paste, then independently interprets the tail as additional chips and ordinary keystrokes.

## What Changes
Retain a detected paste across bounded input passes, emitting it only at the existing idle boundary. Preserve the complete text and one foldable insertion, keep each pass bounded, and flush a pending paste even if no further event arrives. Real bracketed paste and clipboard reads retain their current handling.
