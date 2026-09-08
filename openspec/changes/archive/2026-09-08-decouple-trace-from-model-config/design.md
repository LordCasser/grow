## Design
Delete only the unused model config construction in the Trace dispatch arm. Existing best-effort common startup config reads are not made fatal or moved. Test async_main in a fresh subprocess with isolated HOME/GROW_HOME and malformed config; assert the loader rejects it but Trace reaches a missing-session diagnostic. No real session export or user config access.
