# Finish session load before releasing execution

## Why

A cold suspended resume releases the tree before the remaining load reconciliation and resume usage boundary. An active Goal can start a new Turn while Pager still shows Loading and suppresses ordinary input. The reported session records execution release and Goal admission almost three minutes before the load response.

## What Changes

Complete the load's recovery and response preparation before releasing the suspended tree. Preserve automatic continuation and the original task owners.
