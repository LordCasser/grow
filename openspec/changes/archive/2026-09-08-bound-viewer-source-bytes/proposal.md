## Why
The newly connected background viewer loader still uses unbounded fs::read and copies any in-memory source size. Deferred execution does not provide a byte budget.
## What Changes
Limit viewer source consumption/copying to50,000,000 non-empty encoded bytes. File sources must be regular files after opening; actual reads stop at limit+1. Preserve valid symlink paths. Existing failure completion settles rejected loads.
