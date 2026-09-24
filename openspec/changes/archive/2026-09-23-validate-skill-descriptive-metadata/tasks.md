## 1. Parser and behavior contract

- [x] 1.1 Add regression coverage reproducing bool/number coercion for name, description, when-to-use, and optional display fields, plus partial allowed-tools list filtering.
- [x] 1.2 Restrict scalar extraction to YAML strings; retain directory/body fallbacks and make allowed-tools list parsing all-or-nothing.
- [x] 1.3 Update the skills user guide to explain supported value types and that allowed-tools remains descriptive metadata.

## 2. Verification and archive

- [x] 2.1 Run focused skills discovery tests and inspect the resulting identity, routing and UI projection assertions.
- [x] 2.2 Run `git diff --check`, `openspec validate --all --strict --no-interactive`, record results, then archive and run full/archive validation.
