## Why
Kitty non-PNG previews decode/convert image bytes through sips or image without a prior pixel-count limit. Header validation and encoded byte caps do not bound conversion dimensions.
## What Changes
Require nonzero dimensions and at most 16,000,000 source pixels before either conversion backend. Oversized conversion returns no preview payload; original attachment bytes remain unchanged. Direct PNG and iTerm pass-through are unchanged because Grow does not decode pixels on those paths.
