# Change: Harden list layout cache boundaries

## Why

`ListLayoutCache` fabricates geometry for invalid item indexes (`virtual_y` returns zero or the index, and `item_height` returns one). Its incremental height append also panics when used with the fixed-height variant. These cache boundaries should represent absent items and support append in either representation.

## What Changes

- Make per-item geometry queries return `None` for indexes outside the cached item count.
- Make incremental append extend fixed item counts and variable height/prefix data without a variant panic.
- Keep unrelated dialog geometry, width invalidation, and prefix-sum overflow questions in the backlog.
