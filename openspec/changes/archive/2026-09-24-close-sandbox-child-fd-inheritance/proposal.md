# Close inherited descriptors at restricted child exec

## Why

The Linux child network seccomp filter rejects new connections and network send syscalls, but a child can still use `write` or `sendfile` on a connected socket inherited from Grow. The current launch hooks do not constrain descriptors that survive `exec`. Shell-state transport intentionally passes internal pipes as fd 3 and fd 4, so a blanket close would break those commands.

## What Changes

- At every Linux launch that installs the child network filter, mark all descriptors above stderr close-on-exec before installing seccomp. Failure to establish this boundary rejects the spawn.
- Preserve only the internal shell-state pipe destinations already mapped by their launch path. Ordinary and streaming shell launches preserve no extra descriptors.
- Verify that a pre-connected TCP descriptor cannot send after exec while the internal state pipes and standard streams still work.
