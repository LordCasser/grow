# Guard live Workflow clears with revisions

The live Pager Workflow projection currently bypasses revision checks for `cleared` updates. A stale positive-revision clear can erase a newer Run projection and its running block. Apply the existing positive-revision ordering rule to clear updates while preserving the established revisionless-clear compatibility behavior.
