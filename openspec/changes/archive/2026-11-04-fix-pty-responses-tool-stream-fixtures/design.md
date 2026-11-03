## Design

The single-call helper delegates to `test_support::sse::responses_api_reasoning_then_tool_call_events`, which already emits `response.output_item.added` before `response.function_call_arguments.delta` and includes the completed function-call item.

The parallel helper keeps its one-response/multiple-calls semantics. For each call it emits the same ordered item-announcement and argument-delta lifecycle, with a distinct output index and item id, then emits all completed function-call items in `response.completed`. The production parser remains unchanged and strict.

Two existing PTY cases select the Responses backend while retaining their scripted Chat twin. Their files live in a `project` directory beneath the fake HOME, and the CLI starts with that directory as its working directory. The fake HOME remains available for configuration, while avoiding the first-run prompt shown when Grow itself is started directly in HOME. This makes the single and parallel fixture paths run through the CLI's actual Responses request route when their ignored PTY tests are executed.

The parallel helper registers its paired streams with `expect_agent_turn_with_responses`, whose foreground matcher excludes title and other sideband requests. The PTY case retains and asserts that expectation, then records concise request diagnostics before checking the merged UI row. It verifies that the follow-up Responses request carries both `function_call_output` items and that both edits reached the fixture file, so a retry, incomplete scripted turn, tool execution failure, or UI projection failure is distinguishable without printing system prompts or full request bodies.

The workspace classifier treats OS temporary-directory trees as non-project locations unless they contain a Git repository. Both fixtures initialize their disposable workspace as a Git repository, using the existing project-classification rule rather than disabling the product picker.
