## Why

`install_child_network_filter` runs in the forked child between fork and exec. Constructing its BPF program with `Vec` may allocate on the heap in this sensitive pre-exec path.

## What Changes

Replace the child-network filter's heap-backed instruction buffer with a fixed-size stack array. Keep all instructions, ordering, and sandbox behavior unchanged.

This change does not alter the archived sandbox-boundary contract, so `.openspec.yaml` sets `skip_specs: true` and adds no delta spec.
