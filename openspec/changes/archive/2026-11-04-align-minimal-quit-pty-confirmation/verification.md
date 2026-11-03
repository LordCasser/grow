# Verification

The third Core CI run failed only `minimal_ctrl_d_confirms_and_quits` in its PTY stage after libraries, Settings, CLI build and three protocol-recovery cases had passed. The second key legally re-armed a one-second confirmation after the first screen observation outlived its window. See [run 41689811275](https://github.com/LordCasser/grow/actions/runs/41689811275) and `/tmp/grow-260-candidate3-core-pty-failure-excerpt.log`.

The fixture still observes the first hint and preserves the existing five-second exit deadline and final exit assertion. It clears the old confirmation with Ctrl+C, waits for that hint to disappear, and writes a fresh pair of Ctrl+D bytes together. Product key handling and the one-second confirmation window are unchanged.

The final debug CLI build passed (`/tmp/grow-260-candidate3-cli.log`). With `NO_COLOR` removed and truecolor enabled, all 32 exact PTY cases selected by the existing Core workflow passed: `/tmp/grow-260-core-pty-results.json`, individual logs `/tmp/grow-260-core-pty-01.log` through `...-32.log`. The repaired focused case is index 1 and executed in 4.63s. This includes both real Responses fixtures, shared clients, nested suspension/cold restore, Plan approval and concurrent restoration. Tests execute sequentially under the existing exact runner; no retry or deadline extension was introduced.

The production source base `fccd91b10e141d59d4ed87ca85629015a2949e55` also passed its full local 18-crate suite (19233 / 0 / 51 ignored), required macOS/Linux/Windows native matrix and OpenSpec CI. Exact-source Core CI on this fixture revision and official publication remain in the release closure.
