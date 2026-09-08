# Design
Extract private LogWriter::from_open_file to express construction from an already owned descriptor. Read File::metadata there, derive Unix dev/ino (non-Unix retains existing presence-only identity). No new state type or maintenance timing change.

Deterministic regression opens old file, replaces/removes pathname before constructing writer, checks old handle identity, then forces normal maintenance and verifies subsequent write is visible. This covers the interleaving without probabilistic file replacement scheduling. No Windows replacement detection improvement or promise of zero loss during the existing two-second maintenance interval.
