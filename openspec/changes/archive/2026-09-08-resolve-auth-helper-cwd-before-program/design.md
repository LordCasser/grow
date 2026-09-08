# Design

After trimming and expanding home, use std::path::absolute for configured cwd. This is lexical absolute resolution, not filesystem canonicalization; execution still reports missing directories/programs through the existing process error path. Resolve relative cwd against the Grow process cwd without changing process-global cwd.

The regression creates a temporary child directory under the test process cwd, configures that directory by relative name, and executes ./token.sh. The script reads a sibling token.txt, proving both program resolution and runtime directory. No real helper or credential is used.
