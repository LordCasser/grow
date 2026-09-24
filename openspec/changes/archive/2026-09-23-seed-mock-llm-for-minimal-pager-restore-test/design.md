# Design

Use the existing `ContentController::seed_llm_config` helper immediately after creating the mock server. It writes the test-only provider and default model into the isolated home. Initialize the isolated home as a Git project, matching the passing `minimal_commits_response_to_scrollback` content-backed PTY fixture. Keep this local to the test so cases that deliberately inspect missing or custom configuration retain their own setup. Include mock-server requests in the turn wait failure to distinguish missing inference from missing rendering.
