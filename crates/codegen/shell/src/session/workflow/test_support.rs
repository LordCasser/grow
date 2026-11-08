//! Process isolation for tests that freeze the live Agent/Skill catalog.

use std::time::Duration;

use ::test_support::{TestProcess, TestProcessConfig, TestSandbox};

const CHILD_TEST: &str = "GROW_WORKFLOW_SANDBOX_TEST";

fn test_name() -> String {
    std::thread::current()
        .name()
        .expect("Workflow sandbox must be entered on the libtest thread")
        .to_owned()
}

/// Return true after the exact test has passed in its own sandbox. Call before
/// constructing fixtures, and return from the outer test when true. Unlike an
/// in-process HOME guard this cannot race another test or cached config paths.
pub(crate) fn run_in_sandbox() -> bool {
    let name = test_name();
    if std::env::var(CHILD_TEST).as_deref() == Ok(name.as_str()) {
        return false;
    }
    let mut sandbox = TestSandbox::new();
    sandbox.set_env(CHILD_TEST, &name);
    run_exact(&name, sandbox);
    true
}

fn run_exact(name: &str, mut sandbox: TestSandbox) {
    // TestSandbox intentionally drops ambient variables. Retain only these
    // test-runner controls, not resource discovery or provider configuration.
    for key in ["RUST_MIN_STACK", "GROW_TEST_TIMEOUT_SCALE"] {
        if let Some(value) = std::env::var_os(key) {
            sandbox.set_env(key, value);
        }
    }
    let name = name.to_owned();
    // Keep the real child deadline independent of a caller's paused Tokio clock
    // and avoid nesting a runtime inside a #[tokio::test] runtime.
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
                command
                    .args(["--exact", &name, "--nocapture", "--test-threads=1"])
                    .current_dir(sandbox.workspace());
                let mut process = TestProcess::spawn(
                    command,
                    &sandbox,
                    TestProcessConfig::new().label(format!("Workflow unit test {name}")),
                )
                .unwrap();
                let status = process
                    .wait_with_deadline(::test_support::scaled(Duration::from_secs(60)))
                    .await
                    .unwrap();
                if status.is_none() {
                    process.close().await.expect("stop timed-out Workflow test");
                }
                let stdout = process.stdout_tail();
                let stderr = process.stderr_tail();
                assert!(
                    status.is_some_and(|status| status.success())
                        && stdout.read_error.is_none()
                        && stderr.read_error.is_none()
                        && stdout
                            .text
                            .contains("test result: ok. 1 passed; 0 failed; 0 ignored;"),
                    "isolated Workflow test {name} failed or did not execute exactly once: \
                     {status:?}\n{}\n{}",
                    stdout.text,
                    stderr.text,
                );
            });
    })
    .join()
    .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

#[cfg(unix)]
#[tokio::test]
async fn sandbox_excludes_ambient_catalog_but_keeps_project_discovery() {
    use super::tracker::{WorkflowAgentCatalogSource, test_runtime_route};

    const POLLUTED: &str = "GROW_WORKFLOW_TEST_POLLUTED";
    const SKILL: &str = "ambient-workflow-test-skill";
    const AGENT: &str = "ambient-workflow-test-agent";
    const WORKFLOW: &str = "project-only";
    const WORKSPACE_SKILL: &str = "ambient-workspace-user-skill";
    let name = test_name();
    let isolated = std::env::var(CHILD_TEST).as_deref() == Ok(name.as_str());
    let polluted = std::env::var_os(POLLUTED).is_some();
    if !isolated && !polluted {
        let mut sandbox = TestSandbox::new();
        let skill_dir = sandbox.grow_home().join("skills").join(SKILL);
        std::fs::create_dir_all(&skill_dir).unwrap();
        let body = "x".repeat(chat_state::MAX_WORKFLOW_INITIAL_MANIFEST_BYTES + 1);
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {SKILL}\ndescription: ambient resource\n---\n{body}\n"),
        )
        .unwrap();
        let agents = sandbox.grow_home().join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::write(
            agents.join(format!("{AGENT}.md")),
            format!("---\nname: {AGENT}\ndescription: ambient agent\n---\nAMBIENT ROLE\n"),
        )
        .unwrap();
        let workflows = sandbox.grow_home().join("workflows");
        std::fs::create_dir_all(&workflows).unwrap();
        std::fs::write(
            workflows.join(format!("{WORKFLOW}.rhai")),
            format!("let meta = #{{ name: \"{WORKFLOW}\", description: \"ambient\" }}; complete(\"ok\");"),
        )
        .unwrap();
        let workspace_skill = sandbox
            .home()
            .join("users/ambient-user/.grow/skills")
            .join(WORKSPACE_SKILL);
        std::fs::create_dir_all(&workspace_skill).unwrap();
        std::fs::write(
            workspace_skill.join("SKILL.md"),
            format!("---\nname: {WORKSPACE_SKILL}\ndescription: ambient workspace resource\n---\nWORKSPACE BODY\n"),
        )
        .unwrap();
        sandbox.set_env(POLLUTED, "1");
        sandbox.set_env("GROW_ROOT", sandbox.home().to_owned());
        sandbox.set_env("GROW_USER", "ambient-user");
        run_exact(&name, sandbox);
        return;
    }

    if polluted {
        // Control: the real capture path can see oversized user resources.
        // The isolation fix must not stub discovery or increase the limit.
        let mut route = test_runtime_route();
        route
            .capture_agent_definitions(&WorkflowAgentCatalogSource::for_test(std::env::temp_dir()))
            .await
            .unwrap();
        assert!(route.agent_definition(AGENT).is_ok());
        assert!(
            route
                .frozen_skills()
                .iter()
                .any(|skill| skill.name == WORKSPACE_SKILL)
        );
        assert!(
            super::registry::WorkflowRegistry::scan(None)
                .resolve_by_name(WORKFLOW)
                .is_ok()
        );
        let skill = route
            .frozen_skills()
            .into_iter()
            .find(|s| s.name == SKILL)
            .unwrap();
        assert!(skill.body.unwrap().len() > chat_state::MAX_WORKFLOW_INITIAL_MANIFEST_BYTES);
        assert!(run_in_sandbox());
        return;
    }

    assert!(!run_in_sandbox());
    assert!(std::env::var_os("GROW_ROOT").is_none());
    assert!(std::env::var_os("GROW_USER").is_none());
    assert!(
        super::registry::WorkflowRegistry::scan(None)
            .resolve_by_name(WORKFLOW)
            .is_err()
    );
    let project = tempfile::tempdir().unwrap();
    let skill_dir = project
        .path()
        .join(".grow/skills/local-workflow-test-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: local-workflow-test-skill\ndescription: local resource\n---\nLOCAL BODY\n",
    )
    .unwrap();
    let agents = project.path().join(".grow/agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(
        agents.join("local-workflow-test-agent.md"),
        "---\nname: local-workflow-test-agent\ndescription: local agent\n---\nLOCAL ROLE\n",
    )
    .unwrap();
    let mut route = test_runtime_route();
    route
        .capture_agent_definitions(&WorkflowAgentCatalogSource::for_test(
            project.path().to_owned(),
        ))
        .await
        .unwrap();
    assert!(route.agent_definition(AGENT).is_err());
    assert!(route.agent_definition("local-workflow-test-agent").is_ok());
    assert!(
        !route
            .frozen_skills()
            .iter()
            .any(|skill| skill.name == SKILL || skill.name == WORKSPACE_SKILL)
    );
    assert!(route.frozen_skills().iter().any(|skill| {
        skill.name == "local-workflow-test-skill" && skill.body.as_deref() == Some("LOCAL BODY\n")
    }));
}
