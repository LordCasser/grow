# Why

The v2.2.0 publication run failed before compiling Linux aarch64 because AlmaLinux's mirror list sent DNF to out-of-sync mirrors whose repository metadata files returned 404. The tag's ten-platform dry run had passed, so this is release infrastructure drift rather than a Grow behavior change.

# What changes

Use AlmaLinux's official repository origin for the existing BaseOS, AppStream, and Extras package sources inside the AlmaLinux 8 release builder. Keep the same packages, container major version, target, and glibc 2.28 baseline. Retry the complete release workflow after validation.

# Capabilities

This is CI tool maintenance. It changes no product behavior or archived behavior contract, so `skip_specs: true` applies.
