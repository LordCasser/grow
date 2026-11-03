# Design

Tests launch the freshly built `grow agent stdio` process through `GrowStdioClient` and drive public ACP methods/extensions. Provider traffic stays on `MockInferenceServer` loopback. Image prompts use typed ACP content blocks. Assertions inspect mock wire requests and persisted session/memory files after reload. This change does not modify production code or behavior.
