# Verification
Read-only source audit on main. No new tests/build, provider call or user-session mutation. Scheduling/admission/MaterializeTimeline ordering establishes the issue; no notification interleaving reproduction yet. target already cleaned, disk previously 65 GiB available.
