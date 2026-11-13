## Release correction

The OHOS release job installed xz, printed the remaining dependency names, then skipped directly to Rust. The first install consumed the list supplied to the shell loop on stdin. A shell test ran the actual dependency loop with a stand-in installer that drains stdin. The original loop installed only xz. Redirecting the installer from `/dev/null` preserved the entire installation order. Bash syntax and the official release preflight are also checked.

The pinned Rust 1.93.1 toolchain compiled and passed the paged copy regression after the stable syntax change in v2.6.11.
