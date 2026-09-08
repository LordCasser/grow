## Why
When title generation fails, the fallback takes ten whitespace-delimited words but can exceed the Timeline 160-character title limit. Long CJK text or a long URL therefore makes both generated and fallback title adoption fail.

## What Changes
Cap fallback titles to the existing character limit after whitespace normalization, preserving the ten-word rule and empty-text default.
