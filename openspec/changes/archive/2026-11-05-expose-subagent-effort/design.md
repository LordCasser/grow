## Existing boundary

`tool_types::TaskToolInput` is the canonical model-facing schema. `TaskTool::run` translates it into `SubagentRuntimeOverrides`; currently that translation always sets `reasoning_effort` to `None`. `resolve_runtime_config` already gives an explicit runtime policy precedence over Agent defaults. The production child runner parses the policy and checks it against the resolved model before sampling. Resume separately pins the durable source selection.

## Change

Add `effort: Option<String>` alongside `model`, with a schema description naming the existing canonical effort tokens and selected-model capability restriction. Pass a provided string as `Some(Some(value))`; omission remains outer `None`. Do not normalize or use sentinel sanitization: `"none"` is a real reasoning effort, while runtime continues to report malformed/unsupported values. JSON null has the same unspecified meaning as other optional tool fields; this interface does not add a separate runtime-unset command.

For a valid `resume_from`, ignore the tool effort alongside the existing model override. This preserves source pinning, including when the supplied override is invalid. Fresh tasks may specify effort with a selected model or with the otherwise resolved model. Add no requirement that `model` be present and no new model-selection policy.

The model-facing schema and existing runtime own this field; no duplicated provider validation, new enum or transport is needed. Shared tool descriptions explain how to pass it. Existing Rust struct fixtures gain `effort: None` mechanically.

## Verification

First demonstrate that a JSON Task call carrying model/effort reaches the backend without effort on the old implementation. Verify explicit high and none values, omission, effort without a model, schema publication, JSON roundtrip and resume pinning. Exercise the real child runner/provider boundary for accepted effort and unsupported/invalid failure using existing mock infrastructure. Validate the default CLI build, format and OpenSpec, then archive the change.
