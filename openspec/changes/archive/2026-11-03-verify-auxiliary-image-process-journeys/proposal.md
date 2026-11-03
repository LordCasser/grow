# Verify auxiliary image process journeys

## Why

The archived image journey audit identified gaps where helper- or actor-level coverage did not prove the public process entry points for coordination, recap, memory flush, and fork/rewind/load combinations.

## What Changes

- Add hermetic real-process ACP journey coverage using loopback inference mocks.
- Extend the typed ACP test client with content-block prompt support so image input is sent without shell-string encoding.

## Contract

This is test maintenance only. It does not change runtime behavior or any archived capability contract.
