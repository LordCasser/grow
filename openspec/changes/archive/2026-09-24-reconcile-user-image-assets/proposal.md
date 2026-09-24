# Reconcile user image assets from durable input references

## Why
Image batches are published before their user message is durably admitted. A failed or interrupted admission can leave a published batch without an owner, while a lost acknowledgement can still mean the message committed. Staging directories can also survive a process crash.

## What changes
After the writer epoch is established, reconcile recognized user-image batch and staging directories against image-file references in the committed physical Timeline. Serialize the sweep with live image preparation and message admission. A failed or unreadable Timeline stops deletion.

## Impact
This adds eventual reclamation for unreferenced session image assets. It preserves any batch referenced by a committed user message, including references obscured from the current Surface by compaction or rewind.
