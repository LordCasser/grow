## Local evidence

Shell library tests passed 4,782 cases and ChatState library tests passed 653. Pager library tests passed 7,646 cases before the final transcript fix. The rebuilt Pager passed 221 targeted transcript, load, reconnect and task-result tests, including the new paged copy and stale binding regression. The release tooling ran 35 Python tests with one skip.

The real macOS PTY comparison tested first resume, typed input and Ctrl+D across three fixed-Surface diagnostic history sizes. The working-state change records the measured limits and remains open.

The official release workflow validates the tag's exact commit, runs its regression gates, and builds and verifies the ten platform archives and checksum manifest.
