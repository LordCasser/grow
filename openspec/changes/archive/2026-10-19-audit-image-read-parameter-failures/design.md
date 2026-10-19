# Design

The test writes a small real PNG into a temporary workspace and reuses one `SharedResources` value for four `ReadFileTool::run` calls: each of the two reported PDF-only parameter shapes must return the existing PDF-only error, while an input retaining only `offset`/`limit` and a bare path must produce `ImageContent`.

No production code, tool schema, or user-visible behavior changes. This is a coverage-only audit, so the change sets `skip_specs: true` instead of introducing a behavior delta.
