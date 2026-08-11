use std::sync::{Arc, Mutex};

use workflow::{AgentResult, BudgetState, WorkflowHostRequest as Request};

fn agent_result(label: &str, output: serde_json::Value) -> AgentResult {
    AgentResult {
        agent_id: label.to_string(),
        success: true,
        output,
        cancelled: false,
        tokens_used: 1,
        duration_ms: 1,
    }
}

fn finding(index: usize) -> serde_json::Value {
    serde_json::json!({
        "statement": format!("Finding {index}"),
        "evidence": format!("Original evidence {index}"),
        "context": format!("Context {index}"),
        "source_title": format!("Original {index}"),
        "source_locator": format!("https://original.example/{index}"),
        "source_type": "domain source",
        "confidence": "high",
    })
}

fn verdict(
    index: usize,
    source_support: bool,
    independent_support: bool,
    conflicting_evidence: bool,
    verifier_locator: &str,
) -> serde_json::Value {
    serde_json::json!({
        "finding_id": format!("finding-{index}"),
        "source_support": source_support,
        "independent_support": independent_support,
        "conflicting_evidence": conflicting_evidence,
        "reason": "checked",
        "evidence": if source_support { "verification evidence" } else { "" },
        "source_title": if source_support { "Verification source" } else { "" },
        "source_locator": verifier_locator,
        "limitation": if independent_support { "" } else { "no independent corroboration" },
    })
}

#[derive(Clone, Copy)]
enum VerificationCase {
    Normal,
    Duplicate,
    Missing,
    Foreign,
}

const DEFAULT_SYNTHESIS: &str = "<core-conclusions>\n- First retained conclusion [S1]\n- Second retained conclusion [S2]\n- Third retained conclusion [S3]\n</core-conclusions>\n<report-body>\nThe evidence supports an adaptive synthesis [S1]. A second finding is source-verified [S2]. The final retained finding has its stated evidence boundary [S3].\n</report-body>";

fn run_scenario(
    case: VerificationCase,
    synthesis_output: &'static str,
) -> (serde_json::Value, String) {
    let resolved = super::registry::resolve_deep_research()
        .expect("private deep-research definition must validate");
    let (host_tx, mut host_rx) = tokio::sync::mpsc::unbounded_channel();
    let artifact = Arc::new(Mutex::new(String::new()));
    let captured = artifact.clone();
    let host = std::thread::spawn(move || {
        while let Some(request) = host_rx.blocking_recv() {
            match request {
                Request::ReserveAgentCalls { reply, .. }
                | Request::ReleaseAgentCalls { reply, .. } => {
                    let _ = reply.send(Ok(()));
                }
                Request::SpawnAgent { opts, reply } => {
                    let label = opts.label.unwrap_or_default();
                    let output = match label.as_str() {
                        "research-planner" => serde_json::json!({
                            "axes": [
                                {
                                    "title": "Evidence",
                                    "objective": "Establish the answer",
                                    "evidence_target": "Authoritative sources",
                                },
                                {
                                    "title": "Evidence",
                                    "objective": "Establish the answer",
                                    "evidence_target": "Duplicate source request",
                                },
                            ],
                        }),
                        "researcher-0" => {
                            let mut findings = vec![finding(0), finding(1), finding(2), finding(3)];
                            if !matches!(case, VerificationCase::Normal) {
                                findings.push(finding(4));
                            }
                            serde_json::json!({
                                "findings": findings,
                                "uncertainties": [],
                            })
                        }
                        "evidence-verifier-0" if matches!(case, VerificationCase::Duplicate) => {
                            serde_json::json!({
                                "verdicts": [
                                    verdict(0, true, true, false, "https://verify.example/0"),
                                    verdict(0, true, true, false, "https://verify.example/0"),
                                    verdict(4, true, true, false, "https://verify.example/4"),
                                ],
                            })
                        }
                        "evidence-verifier-0" if matches!(case, VerificationCase::Missing) => {
                            serde_json::json!({
                                "verdicts": [
                                    verdict(4, true, true, false, "https://verify.example/4"),
                                ],
                            })
                        }
                        "evidence-verifier-0" if matches!(case, VerificationCase::Foreign) => {
                            serde_json::json!({
                                "verdicts": [
                                    verdict(0, true, true, false, "https://verify.example/0"),
                                    verdict(4, true, true, false, "https://verify.example/4"),
                                    verdict(99, true, true, false, "https://verify.example/99"),
                                ],
                            })
                        }
                        "evidence-verifier-0" => serde_json::json!({
                            "verdicts": [
                                verdict(0, true, true, false, "https://verify.example/0")
                            ],
                        }),
                        "evidence-verifier-1" => serde_json::json!({
                            "verdicts": [
                                verdict(1, true, false, false, "https://original.example/1")
                            ],
                        }),
                        "evidence-verifier-2" => serde_json::json!({
                            "verdicts": [
                                verdict(2, true, false, true, "https://counter.example/2")
                            ],
                        }),
                        "evidence-verifier-3" => serde_json::json!({
                            "verdicts": [verdict(3, false, false, false, "")],
                        }),
                        "report-synthesizer" => serde_json::json!(synthesis_output),
                        other => panic!("unexpected agent label: {other}"),
                    };
                    let _ = reply.send(Ok(agent_result(&label, output)));
                }
                Request::BudgetQuery { reply } => {
                    let _ = reply.send(Ok(BudgetState {
                        total: None,
                        spent: 0,
                        reserved: 0,
                        remaining: None,
                    }));
                }
                Request::WriteScratchFile { content, reply, .. } => {
                    *captured.lock().unwrap() = content;
                    let _ = reply.send(Ok("scratch/report.md".to_string()));
                }
                Request::RenderTemplate { reply, .. } => {
                    let _ = reply.send(Ok(String::new()));
                }
                Request::ReadScratchFile { reply, .. } => {
                    let _ = reply.send(Ok(String::new()));
                }
                Request::GitDiffSince { reply, .. } => {
                    let _ = reply.send(Ok(String::new()));
                }
                Request::Phase { .. } | Request::Log { .. } | Request::Diagnostic { .. } => {}
            }
        }
    });

    let outcome = workflow::run_workflow(workflow::WorkflowRunParams {
        script: resolved.script,
        args: serde_json::json!({ "query": "Test query" }),
        journal: workflow::Journal::new(None),
        host_tx,
        cancel: tokio_util::sync::CancellationToken::new(),
        max_ops: workflow::WorkflowRunParams::DEFAULT_MAX_OPS,
    });
    host.join().unwrap();
    let workflow::WorkflowOutcome::Completed { result } = outcome else {
        panic!("unexpected workflow outcome: {outcome:?}");
    };
    let full_report = artifact.lock().unwrap().clone();
    (result, full_report)
}

#[test]
fn evidence_levels_are_preserved_without_false_partial_status() {
    let (result, full_report) = run_scenario(VerificationCase::Normal, DEFAULT_SYNTHESIS);
    assert_eq!(result["status"], "verified");
    assert_eq!(result["verified_claim_ids"].as_array().unwrap().len(), 3);
    let chat = result["report"].as_str().unwrap();
    assert!(chat.contains("Research axes:\n  - Evidence — covered"));
    assert!(chat.contains("Independently corroborated: 1"));
    assert!(chat.contains("Source verified without independent corroboration: 2"));
    assert!(chat.contains("Retained with material conflict: 1"));
    assert!(chat.contains("Unsupported findings excluded: 1"));
    assert!(full_report.contains("https://original.example/0"));
    assert!(!full_report.contains("[S4]"));
}

#[test]
fn malformed_verdict_ids_remain_local_to_the_affected_finding() {
    for case in [VerificationCase::Duplicate, VerificationCase::Missing] {
        let (result, full_report) = run_scenario(case, DEFAULT_SYNTHESIS);
        assert_eq!(result["status"], "verified");
        let retained = result["verified_claim_ids"].as_array().unwrap();
        assert_eq!(retained.len(), 3);
        assert!(!retained.iter().any(|id| id.as_str() == Some("finding-0")));
        assert!(retained.iter().any(|id| id.as_str() == Some("finding-4")));
        let chat = result["report"].as_str().unwrap();
        assert!(chat.contains("Verification contract errors: 1"));
        assert!(full_report.contains("Finding finding-0 was excluded"));
    }

    let (result, full_report) = run_scenario(VerificationCase::Foreign, DEFAULT_SYNTHESIS);
    assert_eq!(result["status"], "verified");
    assert_eq!(result["verified_claim_ids"].as_array().unwrap().len(), 4);
    let chat = result["report"].as_str().unwrap();
    assert!(chat.contains("Verification contract errors: 1"));
    assert!(full_report.contains("returned foreign finding ID finding-99"));
}

#[test]
fn adaptive_report_may_change_structure_and_omit_unused_sources() {
    let synthesis = "<core-conclusions>\n核心判断只需要第一项证据 [S1]\n</core-conclusions>\n<report-body>\n# 领域自适应标题\n\n这里采用适合问题的叙事结构，而不是固定综述模板 [S1]。\n</report-body>";
    let (result, full_report) = run_scenario(VerificationCase::Normal, synthesis);
    assert_eq!(result["status"], "verified");
    assert!(full_report.contains("# 领域自适应标题"));
    assert!(full_report.contains("https://original.example/0"));
    assert!(!full_report.contains("https://original.example/1"));
    assert!(!full_report.contains("https://original.example/2"));
}

#[test]
fn invalid_citations_or_unverified_media_use_detailed_fallback() {
    let invalid_outputs = [
        "<core-conclusions>Unknown numeric citation [S99]</core-conclusions><report-body>Supported body [S1]</report-body>",
        "<core-conclusions>Malformed citation [Sunknown]</core-conclusions><report-body>Supported body [S1]</report-body>",
        "<core-conclusions>Supported summary [S1]</core-conclusions><report-body>![unverified](https://unverified.example/image.png) [S1]</report-body>",
        "<core-conclusions>Supported summary [S1]</core-conclusions><report-body>![unverified][image]\n[image]: https://unverified.example/image.png [S1]</report-body>",
        "<core-conclusions>Supported summary [S1]</core-conclusions><report-body><IMG src=\"https://unverified.example/image.png\"> [S1]</report-body>",
        "report without the required synthesis blocks",
    ];
    for synthesis in invalid_outputs {
        let (result, full_report) = run_scenario(VerificationCase::Normal, synthesis);
        assert_eq!(result["status"], "partial");
        assert!(full_report.contains("## Research axis 1"));
        assert!(full_report.contains("detailed evidence fallback is shown instead"));
        assert!(!full_report.contains("unverified.example"));
    }
}
