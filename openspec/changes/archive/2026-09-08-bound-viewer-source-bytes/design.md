## Design
Extract the existing bounded drop-file opening/reading logic into a private shared helper without changing its flags or classification gates. Both drop and viewer sources then use the same actual-read bound; Unix O_NONBLOCK prevents FIFO opening from blocking and same-handle metadata rejects nonregular sources. No O_NOFOLLOW: valid explicit symlink behavior remains.

The public background loader supplies a50MB ceiling to a private implementation accepting a limit for deterministic tests. Memory source length is checked before to_vec. Header validation and protocol-specific conversion run only after source admission. The dormant synchronous open_from_path candidate R20 is unchanged and is not a production caller of this loader.
## Limits
The bound covers new source reads/copies, not already allocated Arc storage, display-conversion allocations, aggregate concurrent loaders or filesystem I/O deadlines. Owner validation and deferred admission are unchanged.
