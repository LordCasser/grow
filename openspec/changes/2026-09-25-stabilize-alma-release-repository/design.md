# Design

Override the three enabled AlmaLinux 8 DNF repositories at the package-install command, keeping their existing GPG settings. Point each to `https://repo.almalinux.org/almalinux/8/<repository>/<arch>/os/` and suppress the mirror list for that invocation. Use the container's architecture for both x86_64 and aarch64. This avoids editing image files and keeps the change local to the AlmaLinux builder.

Verify the official repository metadata exists for aarch64, exercise the DNF command in an AlmaLinux 8 aarch64 container, check workflow syntax and OpenSpec, then rerun the full publication workflow. Archive this record only after successful publication.
