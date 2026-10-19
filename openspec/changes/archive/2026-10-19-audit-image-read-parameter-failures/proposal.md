# Audit image read parameter failures

## Why

An audit found that `read_file` correctly treats `pages` and `format` as PDF-only, but there was no end-to-end PNG regression covering both rejected parameter shapes and a successful retry without those fields.

## What Changes

Add a real PNG `ReadFileTool::run` regression using the existing local filesystem fixture. The regression will assert both PDF-only parameter errors and successful image reads after removing those fields. This records existing behavior; it does not change the product contract or implementation.
