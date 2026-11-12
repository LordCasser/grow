# Verification

The v2.6.8 Linux gate passed the interrupted Tasks PTY case. In the Goal PTY case the restored session showed Goal Paused, an empty composer and its command hint. The once-typed restart draft never appeared and the test had yet to press Enter. The existing Pager test `loading_replay_ignores_input_and_preserves_existing_draft_until_session_loaded` confirms that Pager ignores ordinary input during replay.

The v2.6.9 PTY fixture passed against the local v2.6.8 CLI, whose runtime code is identical. It observed the exact restart draft before submitting it once. The Goal-owned continuation count rose from one to two and the Goal completed without another user prompt. The official gates run the corrected fixture against the v2.6.9 CLI.
