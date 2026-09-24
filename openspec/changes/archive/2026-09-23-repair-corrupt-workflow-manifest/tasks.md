## 1. Guarded repair and acknowledgement

- [x] 1.1 Carry a fixed-size fingerprint for invalid sidecars through restore and verify the storage repair path refuses changed/missing snapshots without weakening normal revision CAS; run `corrupt_manifest_repair_replaces_only_the_observed_snapshot` and `concurrent_workflow_manifest_writes_converge_on_highest_revision`.
- [x] 1.2 Route corrupt-seed repair through persistence ACK and propagate write/ack failure from restored actor initialization; run `corrupt_sidecar_repair_waits_for_its_storage_ack` and `corrupt_workflow_repair_ack_reports_durable_write_and_stale_snapshot`.

## 2. Contract and validation

- [x] 2.1 Update Workflow JSONL restore regression coverage and developer architecture notes; run `workflow_restore_rebuilds_missing_and_invalid_manifests_from_timeline` and the focused shell tests.
- [x] 2.2 Run `openspec validate --all --strict --no-interactive`, review the delta against the archived Workflow contract, archive this change, and run full active/archive validation.
