## Correctness

The focused Shell branch publication tests pass for prompt-coordinate reuse, selected recent and older pages, the original discarded Turn identity, and a provider image description linked to its original Sideband and image leaf. The image fixture checks current item coordinates and the original image and ledger in the full audit.

The process-loss regression exits the real writer before and after manifest replacement for both rewind and image projection. In the rewind cases the canonical event exists and the display marker has yet to be written. A torn record is appended before recovery. The test confirms the same Surface IDs and revision as dense replay, exact retry without a duplicate, and indexed recovery on the subsequent open. This regression exposed a journal-stamp mismatch after truncation restored the published byte length. The writer now updates the stamp when it verifies that an exact retry removed only a torn tail at that boundary.

The initial working-snapshot selection passed 41 tests with the opt-in large publication probe excluded. The final branch selection passed three focused tests. The broader ChatState library run passed 660 tests with three opt-in cases excluded. The first Shell library run passed 4,830 tests and failed the file watcher timing test while builds were running. The watcher passed a separate exact run. This failure was also observed intermittently during the previous change.

The Pager library suite passed 7,651 tests with 15 opt-in cases excluded. The release-distribution candidate was built on arm64 macOS using Rust 1.93.1 and `cargo build --locked -p cli --profile release-dist --features release-dist`.

## Terminal A/B setup

The baseline is the official `grow-aarch64-apple-darwin` binary from Grow v2.6.13 (`f617188835dc2d387600e28b4595ed99c9cd36fa`), verified against the downloaded release checksum. The candidate is the release-distribution build above. Their binary SHA-256 digests are `e48265492f0a4a6ffb20ec820855eb6d5ae77b4516860158f27ababc79354ccd` and `e761e3b2b9c13e291b7cd0080cf0f5889fd6adb2edd94efbcf08c9294b964942`, respectively.

Each arm used its own normal Shell writer and an isolated home. The preflight validates the selected Surface with dense replay, one selected user/assistant pair plus the fixed seed, and compares the BLAKE3 fingerprint across all sizes and arms. The arm journals retain the raw cancelled branch or original image. Each writer publishes a fixed post-transformation diagnostic record before leaving two complete, fsynced Observation records outside its manifest. The two tail records are byte-identical within each size. Comparing across sizes normalizes the sequence fields and their checksums. Closed Tool lifecycle pairs are the only scale variable. The runner prepares every fixture before timing and rotates arm and size order across three paired trials per size.

The operation run sends actual keys into the fullscreen app PTY and verifies frames from the terminal stream. The mock provider serves only explicit prompt submissions. Timings from process launch record the first complete frame, latest-history marker, and the echo of typed input. Operation roundtrip starts at submission after the recent history appears. The older-page measurement uses a separate process resume of the same session after one identical explicit prompt. The idle control opens a fresh prepared fixture, submits no prompts, waits for readiness and the latest page, then measures from confirmed Ctrl+D through process exit.

## Rewind

Medians over three trials per arm, in milliseconds. Each cell reads v2.6.13 → candidate. The newest history page is measured separately from the first terminal frame, which can be the loading frame.

| Closed Tool pairs | Journal bytes | First frame | Latest history | Input echo | First operation | Older page | Idle Ctrl+D |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 64 | 2,239,118 | 122.8 → 108.8 | 224.6 → 218.5 | 287.3 → 218.5 | 207.5 → 217.1 | 298.2 → 40.1 | 119.5 → 139.3 |
| 8,192 | 141,645,760 | 108.1 → 85.7 | 937.0 → 208.7 | 1,061.2 → 208.7 | 237.2 → 218.6 | 649.8 → 40.1 | 119.7 → 135.3 |
| 30,000 | 515,740,193 | 105.4 → 83.9 | 2,889.5 → 204.1 | 3,189.8 → 210.1 | 221.3 → 217.3 | 1,541.5 → 40.1 | 124.6 → 135.7 |

The rewind Surface digest was `ef62f3959af0889ada09a43953e37faf532ac9c1d896dbc52292b88c860e8352`. The normalized suffix digest was `40cbd65217b20c93c2639c1a96a9fc971499a36b85f2956a9e11ea524dc58774`.

The candidate hydrated two retained records totaling 514–518 B, and read a 652–660 B unpublished suffix on every initial open. The two tail records are unchanged. The sequence positions cross a decimal width boundary between the first two sizes. First-open working hydration medians were 1.45, 3.00, and 2.15 ms. The baseline fell back to dense Timeline reading after checking its old index. Its largest first-open `session.load_light` median was 2,713 ms, compared with 45 ms in the candidate. At repeat open the candidate rendered four recent rows from the new turn and read 128 selected older rows when requested. The older-page RPC median on the largest baseline was 1,259 ms. Candidate older-page reads took at most 1.38 ms internally across all three sizes.

All rewind PTY exits returned code 0. The initial opens recorded zero exit retries. On the idle control, all 18 opens had zero foreground inference requests. Idle exit was roughly 11–20 ms slower in the candidate relative to v2.6.13 across the three medians and did not trend upward with history length.

## Unsupported image projection

Medians over three trials per arm, in milliseconds. Each cell reads v2.6.13 → candidate.

| Closed Tool pairs | Journal bytes | First frame | Latest history | Input echo | First operation | Older page | Idle Ctrl+D |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 64 | 2,234,940 | 99.5 → 93.9 | 199.3 → 192.3 | 240.8 → 205.3 | 208.0 → 201.9 | 311.5 → 311.0 | 119.4 → 135.1 |
| 8,192 | 141,641,534 | 90.3 → 79.5 | 924.7 → 215.5 | 1,038.0 → 228.7 | 211.7 → 214.9 | 671.8 → 302.3 | 120.0 → 134.5 |
| 30,000 | 515,735,967 | 98.2 → 72.8 | 2,876.2 → 194.8 | 3,183.4 → 207.8 | 235.5 → 208.1 | 1,553.2 → 302.3 | 125.4 → 134.8 |

The projected Surface digest was `9849aa52b500ae51b377b2af59b872665cb5662bb49f9af8a20fdc0c3bdc3a91`. Its normalized unpublished suffix digest matched the rewind fixture. All exact suffix digests matched between the operation and idle runs at each size.

The candidate hydrated one retained record totaling 257–259 B, verified one exact projection reference, and read the same 652–660 B unpublished suffix. First-open working hydration medians were 1.70, 3.28, and 3.05 ms. At the largest size, first-open `session.load_light` was 2,711 ms in the baseline and 40 ms in the candidate. The candidate read a recent page of 128 rows and returned 22 older rows on request. On the largest baseline, the older-page RPC took 1,246 ms median. Candidate older-page reads took at most 3.40 ms across all three sizes. The highest recent-page read on repeat open was 5.63 ms. The fullscreen older-page timing includes the PageUp navigation through a filled recent page. In the traced largest trial approximately 300 ms passed before the older-page RPC, whose apply stage took 0.14 ms.

## Acceptance

Both transformations passed all three sizes and both probes, including 108 real PTY launches with successful exits. The 72 initial opens recorded zero exit retries. The 36 idle launches submitted zero foreground requests. The final optimized harness passed the ordinary and transformed fixture-writer tests. The pinned baseline writer also passed the transformed fixture test. The candidate's first screen with recent history, input echo, first operation, older-page read and confirmed idle exit show no increase proportional to closed Tool lifecycle history. The candidate's idle Ctrl+D medians were about 9–20 ms higher than the baseline, with no upward size trend for either transformation. The validated size curve covers a fixed working branch and two durable unpublished facts. Explicit transformation commits and one-time repair without a compatible working publication still use canonical validation of the prefix.
