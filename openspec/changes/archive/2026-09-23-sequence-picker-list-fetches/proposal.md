# Change: Sequence session picker list fetches

## Why

The picker accepts every plain list response under the same sequence number. Two rapid opens can display the older result last. Closing an Agent modal also leaves its pending response eligible to fall through into the welcome picker fields.

## What Changes

- Advance the list generation for every fetch and every picker dismissal.
- Apply a current result only to a visible picker surface; do not write a hidden welcome picker after an Agent modal disappears.
- Keep the request's query and deep-search indicators independent.

## Impact

Pager session picker result routing. Directory selection and relaxed-scope notice ownership remain separate backlog work.
