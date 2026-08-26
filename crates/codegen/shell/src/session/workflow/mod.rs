pub(crate) mod host_service;
pub(crate) mod manager;
pub(crate) mod notify;
pub(crate) mod registry;
pub(crate) mod schema_contract;
pub(crate) mod store;
pub(crate) mod tracker;
pub(crate) mod workspace;

#[cfg(test)]
mod deep_research_tests;

#[cfg(test)]
mod managed_workflow_tests {
    #[test]
    fn extracted_deep_research_uses_the_user_workflow_registry() {
        let home = tempfile::tempdir().unwrap();
        crate::builtin::extract_builtin_files(home.path());
        let resolved = super::registry::WorkflowRegistry::scan_with_user_root(None, home.path())
            .resolve_by_name("deep-research")
            .expect("managed deep-research definition must validate");
        let script = resolved.script;
        assert_eq!(resolved.meta.name, "deep-research");
        assert_eq!(
            resolved.scope,
            tools::implementations::grow_build::workflow::WorkflowScope::User
        );
        assert_eq!(resolved.definition_id.0, "user:deep-research");
        assert!(matches!(
            resolved.source,
            super::registry::WorkflowSource::File(_)
        ));
        let validation = workflow::validate_script(
            &script,
            Some(serde_json::json!({ "query": "validation probe" })),
        )
        .expect("managed deep-research workflow must dry-run");
        assert!(validation.outcome_ok);
        assert!(script.contains("let breadth = 6"));
        assert!(script.contains("let candidate_cap = 48"));
        assert!(script.contains("let verifier_count = 4"));
        assert!(script.contains("source_support"));
        assert!(script.contains("independent_support"));
        assert!(script.contains("conflicting_evidence"));
        assert!(script.contains("if matches.len() != 1"));
        assert!(!script.contains("let shard_valid"));
        assert!(script.contains("verified_claim_ids"));
        assert!(script.contains("**Status: Partial**"));
        assert!(script.contains("label: \"report-synthesizer\""));
        assert!(script.contains("<core-conclusions>"));
        assert!(script.contains("<report-body>"));
        assert!(script.contains("citations_valid"));
        assert!(script.contains("external_links_valid"));
        assert!(script.contains("external_images_valid"));
        assert!(script.contains("let report_fallback"));
        assert!(script.contains("/deep-research <query>"));
        assert!(script.contains("/workflow-run deep-research <query>"));
        assert!(!script.contains("/workflow deep-research"));
        assert!(script.contains("You may omit redundant or peripheral findings"));
        assert!(script.contains("Never invent or renumber a marker, source, fact"));
        assert!(!script.contains("keep the whole body concise"));
        assert!(!script.contains("Cite every packet entry at least once"));
        assert!(script.contains("report: chat_report"));
    }
}
