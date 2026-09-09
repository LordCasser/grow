## Why
Mid-turn input displays raw text live but persists the LLM envelope into display updates, exposing internal instructions after resume.

## What Changes
Persist the original sanitized user text for interjection display, while retaining wrapped text and skill expansion in the model Timeline. No timeline migration or prompt-string parsing.
