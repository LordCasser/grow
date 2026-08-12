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
mod builtin_tests {
    #[test]
    fn every_builtin_validates_and_matches_its_registered_name() {
        for builtin in super::registry::BUILTIN_WORKFLOWS {
            let meta = workflow::extract_meta(builtin.script)
                .unwrap_or_else(|e| panic!("builtin '{}' must validate: {e}", builtin.name));
            assert_eq!(
                meta.name, builtin.name,
                "registry key must equal meta.name for '{}'",
                builtin.name
            );
        }
    }

    #[test]
    fn deep_research_uses_adaptive_axes_and_per_finding_verification() {
        let resolved = super::registry::resolve_deep_research()
            .expect("private deep-research definition must validate");
        let script = resolved.script;
        assert_eq!(resolved.meta.name, "deep-research");
        assert!(super::registry::BUILTIN_WORKFLOWS.is_empty());
        let validation = workflow::validate_script(
            &script,
            Some(serde_json::json!({ "query": "validation probe" })),
        )
        .expect("private deep-research workflow must dry-run");
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
        assert!(script.contains("You may omit redundant or peripheral findings"));
        assert!(script.contains("Never invent or renumber a marker, source, fact"));
        assert!(!script.contains("keep the whole body concise"));
        assert!(!script.contains("Cite every packet entry at least once"));
        assert!(script.contains("report: chat_report"));
    }
}
