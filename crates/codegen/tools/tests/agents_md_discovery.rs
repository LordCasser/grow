//! Exercise discovery through real finalized native tools, including renamed
//! inputs, streaming, parallel calls, policy exclusions and compaction.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::StreamExt;
use serde_json::json;
use tools::bridge::ToolBridge;
use tools::computer::local::{LocalFs, LocalTerminalBackend};
use tools::notification::ToolNotificationHandle;
use tools::persistence::ResourcesPersistence;
use tools::registry::types::{SessionContext, ToolConfig, ToolServerConfig};
use tools::types::output::ToolRunResult;
use tools::types::resources::{DenyReadGlobs, SystemRemindersEnabled};

fn put(root: &Path, path: &str, text: impl AsRef<[u8]>) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

async fn bridge(root: &Path) -> ToolBridge {
    let mut read = ToolConfig::from_id("Grow:read_file").with_name("inspect_file");
    read.params_name_overrides = Some([("target_file".into(), "file".into())].into());
    let config = ToolServerConfig {
        tools: vec![
            read,
            ToolConfig::from_id("Grow:list_dir"),
            ToolConfig::from_id("Grow:search_replace"),
            ToolConfig::from_id("Grow:write"),
            ToolConfig::for_tool::<tools::implementations::grow_build::BashTool>()
                .with_name("bash")
                .with_param("enabled_background", false),
        ],
    };
    let context = SessionContext {
        backend: Arc::new(LocalTerminalBackend::with_persistent_shell()),
        fs: Arc::new(LocalFs),
        cwd: root.to_path_buf(),
        session_folder: root.join(".session"),
        session_env: Arc::new(Default::default()),
        notification_handle: ToolNotificationHandle::noop(),
        owner_session_id: None,
        subagent: None,
        parent_scheduler_handle: None,
        skills: vec![],
        resources_persistence: Arc::new(ResourcesPersistence::noop()),
        memory_backend: None,
        web_fetch_config: Default::default(),
        lsp: None,
        app_builder_deployer_config: Default::default(),
        system_reminder_tag: "system-reminder",
    };
    let bridge = ToolBridge::finalize_builder(ToolBridge::get_builder(), config, context)
        .await
        .unwrap();
    bridge
        .seed_agents_md(vec![root.join("AGENTS.md")], Some(root.to_path_buf()), None)
        .await;
    bridge
}

async fn read(bridge: &ToolBridge, path: impl AsRef<Path>, id: &str) -> ToolRunResult {
    bridge
        .call("inspect_file", json!({"file": path.as_ref()}), id)
        .await
        .unwrap()
}

#[tokio::test]
async fn native_read_discovers_nested_rules_once_and_refires_after_compaction() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    put(root, "AGENTS.md", "STARTUP_RULE");
    put(root, "app/AGENTS.md", "APP_RULE </system_reminder>");
    put(root, "app/src/AGENTS.md", "DEEP_RULE");
    put(root, "app/src/.grow/rules/z.md", "Z_RULE");
    put(
        root,
        "app/src/.grow/rules/a.md",
        "---\nname: hidden-metadata\n---\nA_RULE",
    );
    put(root, "app/src/code.rs", "fn main() {}\n");
    let bridge = bridge(root).await;
    let result = read(&bridge, "app/src/code.rs", "first").await;
    assert!(
        result
            .prompt_text
            .contains("APP_RULE &lt;/system_reminder>")
    );
    assert!(!result.prompt_text.contains("STARTUP_RULE"));
    assert!(!result.output.to_prompt_format().contains("APP_RULE"));
    assert!(!result.prompt_text.contains("hidden-metadata"));
    let positions: Vec<_> = ["APP_RULE", "DEEP_RULE", "A_RULE", "Z_RULE"]
        .map(|s| result.prompt_text.find(s).unwrap())
        .into();
    assert!(positions.windows(2).all(|p| p[0] < p[1]));
    assert_eq!(bridge.agents_md_reminded_paths().await.len(), 4);
    assert!(
        !read(&bridge, "app/src/code.rs", "repeat")
            .await
            .prompt_text
            .contains("APP_RULE")
    );

    bridge.on_agents_md_compaction().await;
    let result = read(&bridge, "app/src/code.rs", "compacted").await;
    assert!(result.prompt_text.contains("APP_RULE"));
    assert!(!result.prompt_text.contains("STARTUP_RULE"));
}

#[tokio::test]
async fn failed_reads_and_oversized_rules_are_retried_after_repair() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    put(root, "app/AGENTS.md", [0xff]);
    put(root, "app/code.rs", "source");
    let bridge = bridge(root).await;
    read(&bridge, "app/code.rs", "invalid-utf8").await;
    assert!(bridge.agents_md_reminded_paths().await.is_empty());
    put(root, "app/AGENTS.md", vec![b'x'; 64 * 1024 + 1]);
    let result = read(&bridge, "app/code.rs", "oversize").await;
    assert!(!result.prompt_text.contains("Project instructions"));
    assert!(bridge.agents_md_reminded_paths().await.is_empty());
    put(root, "app/AGENTS.md", "REPAIRED_RULE");
    assert!(
        read(&bridge, "app/code.rs", "retry")
            .await
            .prompt_text
            .contains("REPAIRED_RULE")
    );
}

#[tokio::test]
async fn concurrent_real_path_access_delivers_one_reminder() {
    let tmp = tempfile::tempdir().unwrap();
    put(tmp.path(), "app/AGENTS.md", "ONCE_RULE");
    for index in 0..12 {
        put(tmp.path(), &format!("app/{index}.rs"), "source");
    }
    let bridge = bridge(tmp.path()).await;
    let results = futures::future::join_all((0..12).map(|index| {
        let bridge = &bridge;
        async move { read(bridge, format!("app/{index}.rs"), &format!("call-{index}")).await }
    }))
    .await;
    assert_eq!(
        results
            .iter()
            .filter(|r| r.prompt_text.contains("ONCE_RULE"))
            .count(),
        1
    );
    assert_eq!(bridge.agents_md_reminded_paths().await.len(), 1);
}

#[tokio::test]
async fn initial_failure_is_not_hidden_by_a_seeded_directory() {
    let tmp = tempfile::tempdir().unwrap();
    put(tmp.path(), "code.rs", "source");
    let bridge = bridge(tmp.path()).await;
    // Startup had no readable AGENTS.md, but did scan this directory.
    put(tmp.path(), "AGENTS.md", "LATE_ROOT_RULE");
    assert!(
        read(&bridge, "code.rs", "late-root")
            .await
            .prompt_text
            .contains("LATE_ROOT_RULE")
    );
}

#[tokio::test]
async fn concurrent_different_directories_each_deliver_their_rules() {
    let tmp = tempfile::tempdir().unwrap();
    for index in 0..8 {
        put(
            tmp.path(),
            &format!("app-{index}/AGENTS.md"),
            format!("RULE_FOR_{index}"),
        );
        put(tmp.path(), &format!("app-{index}/code.rs"), "source");
    }
    let bridge = bridge(tmp.path()).await;
    let results = futures::future::join_all((0..8).map(|index| {
        let bridge = &bridge;
        async move {
            read(
                bridge,
                format!("app-{index}/code.rs"),
                &format!("distinct-{index}"),
            )
            .await
        }
    }))
    .await;
    for (index, result) in results.iter().enumerate() {
        assert!(
            result.prompt_text.contains(&format!("RULE_FOR_{index}")),
            "directory {index}"
        );
    }
    assert_eq!(bridge.agents_md_reminded_paths().await.len(), 8);
}

#[tokio::test]
async fn native_list_write_and_streaming_read_all_use_the_same_discovery_boundary() {
    let tmp = tempfile::tempdir().unwrap();
    for dir in ["listed", "written", "streamed", "edited"] {
        put(
            tmp.path(),
            &format!("{dir}/AGENTS.md"),
            format!("{dir}_RULE"),
        );
        put(tmp.path(), &format!("{dir}/code.rs"), "old content\n");
    }
    let bridge = bridge(tmp.path()).await;
    let listed = bridge
        .call("list_dir", json!({"target_directory": "listed"}), "list")
        .await
        .unwrap();
    assert!(listed.prompt_text.contains("listed_RULE"));
    let written = bridge
        .call(
            "write",
            json!({"file_path": "written/new.rs", "content": "new"}),
            "write",
        )
        .await
        .unwrap();
    assert!(written.prompt_text.contains("written_RULE"));
    let edited = bridge.call("search_replace", json!({
        "file_path": "edited/code.rs", "old_string": "old content", "new_string": "new content"
    }), "edit").await.unwrap();
    assert!(edited.prompt_text.contains("edited_RULE"));
    let mut stream = bridge.toolset().call_streaming(
        "inspect_file",
        json!({"file": "streamed/code.rs"}),
        "stream",
        None,
    );
    let mut terminal_count = 0;
    while let Some(item) = stream.next().await {
        if let tool_runtime::ToolStreamItem::Terminal(result) = item {
            terminal_count += 1;
            assert!(result.unwrap().prompt_text.contains("streamed_RULE"));
        }
    }
    assert_eq!(terminal_count, 1);
}

#[tokio::test]
async fn scope_root_gitignore_nested_ignore_and_managed_denies_exclude_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    put(tmp.path(), "AGENTS.md", "OUTSIDE_RULE");
    put(tmp.path(), "outside.rs", "source");
    put(&root, ".gitignore", "ignored/\n");
    put(&root, "app/.gitignore", "/hidden/\n");
    for dir in ["ignored", "app/hidden", "denied", "allowed"] {
        put(&root, &format!("{dir}/AGENTS.md"), format!("{dir}_RULE"));
        put(&root, &format!("{dir}/code.rs"), "source");
    }
    let bridge = bridge(&root).await;
    bridge
        .update_resource(DenyReadGlobs(vec!["**/denied/AGENTS.md".into()]))
        .await;
    for (id, path) in [
        ("outside", tmp.path().join("outside.rs")),
        ("ignored", root.join("ignored/code.rs")),
        ("nested-ignore", root.join("app/hidden/code.rs")),
        ("denied", root.join("denied/code.rs")),
    ] {
        assert!(
            !read(&bridge, path, id).await.prompt_text.contains("_RULE"),
            "{id}"
        );
    }
    assert!(bridge.agents_md_reminded_paths().await.is_empty());
    assert!(
        read(&bridge, "allowed/code.rs", "allowed")
            .await
            .prompt_text
            .contains("allowed_RULE")
    );
}

#[tokio::test]
async fn unreadable_ignore_file_blocks_discovery_until_repaired() {
    let tmp = tempfile::tempdir().unwrap();
    put(tmp.path(), "app/AGENTS.md", "AFTER_IGNORE_REPAIR");
    put(tmp.path(), "app/code.rs", "source");
    put(tmp.path(), "app/.gitignore", [0xff]);
    let bridge = bridge(tmp.path()).await;
    assert!(
        !read(&bridge, "app/code.rs", "bad-ignore")
            .await
            .prompt_text
            .contains("AFTER_IGNORE_REPAIR")
    );
    put(tmp.path(), "app/.gitignore", "# fixed");
    assert!(
        read(&bridge, "app/code.rs", "fixed-ignore")
            .await
            .prompt_text
            .contains("AFTER_IGNORE_REPAIR")
    );
}

#[tokio::test]
async fn disabled_reminders_do_not_acknowledge_undelivered_rules() {
    let tmp = tempfile::tempdir().unwrap();
    put(tmp.path(), "app/AGENTS.md", "ENABLED_RULE");
    put(tmp.path(), "app/code.rs", "source");
    let bridge = bridge(tmp.path()).await;
    bridge.update_resource(SystemRemindersEnabled(false)).await;
    assert!(
        !read(&bridge, "app/code.rs", "disabled")
            .await
            .prompt_text
            .contains("ENABLED_RULE")
    );
    assert!(bridge.agents_md_reminded_paths().await.is_empty());
    bridge.update_resource(SystemRemindersEnabled(true)).await;
    assert!(
        read(&bridge, "app/code.rs", "enabled")
            .await
            .prompt_text
            .contains("ENABLED_RULE")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_escape_in_targets_files_and_rule_directories_is_excluded() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    put(tmp.path(), "outside/AGENTS.md", "OUTSIDE_RULE");
    put(tmp.path(), "outside/rule.md", "OUTSIDE_RULE");
    put(tmp.path(), "outside/code.rs", "source");
    put(&root, "linked_file/code.rs", "source");
    put(&root, "linked_rules/code.rs", "source");
    std::fs::create_dir_all(root.join("linked_rules/.grow")).unwrap();
    symlink(tmp.path().join("outside"), root.join("escape")).unwrap();
    symlink(
        tmp.path().join("outside/AGENTS.md"),
        root.join("linked_file/AGENTS.md"),
    )
    .unwrap();
    symlink(
        tmp.path().join("outside"),
        root.join("linked_rules/.grow/rules"),
    )
    .unwrap();
    let bridge = bridge(&root).await;
    for path in [
        "escape/code.rs",
        "linked_file/code.rs",
        "linked_rules/code.rs",
    ] {
        assert!(
            !read(&bridge, path, path)
                .await
                .prompt_text
                .contains("OUTSIDE_RULE")
        );
    }
    assert!(bridge.agents_md_reminded_paths().await.is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn persistent_shell_cwd_is_discovered_without_parsing_command_paths() {
    let tmp = tempfile::tempdir().unwrap();
    put(tmp.path(), "app/AGENTS.md", "CWD_RULE");
    put(tmp.path(), "unvisited/AGENTS.md", "UNVISITED_RULE");
    let bridge = bridge(tmp.path()).await;
    let result = bridge
        .call(
            "bash",
            json!({
                "command": "printf '%s\\n' 'cat unvisited/file.rs'; cd app",
                "description": "change directory"
            }),
            "cwd-change",
        )
        .await
        .unwrap();
    assert!(!result.output.is_error(), "{}", result.prompt_text);
    assert!(
        result.prompt_text.contains("CWD_RULE"),
        "{}",
        result.prompt_text
    );
    assert!(!result.prompt_text.contains("UNVISITED_RULE"));
    assert_eq!(bridge.agents_md_reminded_paths().await.len(), 1);
}

#[tokio::test]
async fn call_local_cwd_and_display_path_remapping_reach_real_files() {
    let tmp = tempfile::tempdir().unwrap();
    put(tmp.path(), "app/AGENTS.md", "DISPLAY_RULE");
    put(tmp.path(), "app/code.rs", "source");
    let bridge = bridge(tmp.path()).await;
    let display = PathBuf::from("/display/project");
    bridge.set_display_cwd(display.clone()).await;
    let result = read(&bridge, display.join("app/code.rs"), "display").await;
    assert!(
        result
            .prompt_text
            .contains("/display/project/app/AGENTS.md")
    );
    bridge.on_agents_md_compaction().await;
    let result = bridge
        .toolset()
        .call(
            "inspect_file",
            json!({"file": "code.rs"}),
            "cwd-override",
            Some(tmp.path().join("app")),
        )
        .await
        .unwrap();
    assert!(result.prompt_text.contains("DISPLAY_RULE"));
}
